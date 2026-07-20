use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    prelude::{StatefulWidget, Widget},
    style::{Color, Modifier, Style},
    widgets::Block,
};
use ratatui_textarea::{TextArea, WrapMode};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

// ── Single-line text input (used by the config tab forms) ──────

/// State for a single-line text input.
///
/// The cursor is stored as a byte offset (so `O(1)` access at the
/// rendering layer can slice the content), but all movement /
/// deletion operations walk **grapheme clusters** so combining marks
/// (e.g. `e` + combining acute `´`) are treated as a single edit unit
/// instead of as two separate bytes.
#[derive(Debug)]
pub struct InputState {
    content: String,
    /// Cursor position as a byte offset into `content`; always lies
    /// on a grapheme cluster boundary (or `0` or `content.len()`).
    cursor: usize,
    /// Optional cap on the number of grapheme clusters (not bytes).
    /// `None` means no cap. Config fields default to no cap; chat
    /// `InputArea` defaults to a generous 16 384-char cap.
    max_length: Option<usize>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            content: String::new(),
            cursor: 0,
            max_length: None,
        }
    }
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn value(&self) -> &str {
        &self.content
    }

    /// Cursor position as a byte offset. Always lies on a grapheme
    /// cluster boundary. Stable to use as a render-time split index.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Total grapheme clusters in the field (for length cap checks).
    pub fn grapheme_len(&self) -> usize {
        self.content.graphemes(true).count()
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor = 0;
    }

    /// Cap the field to at most `n` grapheme clusters. Pasted content
    /// longer than the cap is truncated. Pass `None` via [`clear_max_length`]
    /// to remove the cap.
    pub fn set_max_length(&mut self, n: usize) {
        self.max_length = Some(n);
    }

    pub fn clear_max_length(&mut self) {
        self.max_length = None;
    }

    pub fn max_length(&self) -> Option<usize> {
        self.max_length
    }

    /// Insert a single character at the cursor. Honours `max_length`.
    /// Returns `true` if the field changed.
    pub fn insert_char(&mut self, ch: char) -> bool {
        if self.exceeds_cap_by(1) {
            return false;
        }
        self.content.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        true
    }

    /// Paste entry point: insert an arbitrary `&str` at the cursor,
    /// truncated to honour `max_length`. Returns `true` if anything
    /// was actually written.
    pub fn insert_str(&mut self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let to_insert: String = if let Some(cap) = self.max_length {
            let current = self.grapheme_len();
            let budget = cap.saturating_sub(current);
            if budget == 0 {
                return false;
            }
            s.graphemes(true).take(budget).collect()
        } else {
            s.to_string()
        };
        if to_insert.is_empty() {
            return false;
        }
        let byte_len = to_insert.len();
        self.content.insert_str(self.cursor, &to_insert);
        self.cursor += byte_len;
        true
    }

    /// Delete the grapheme cluster immediately before the cursor (i.e.
    /// the Backspace key). Combining-mark sequences are removed in a
    /// single press.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = previous_grapheme_boundary(&self.content, self.cursor);
        self.content.drain(prev..self.cursor);
        self.cursor = prev;
    }

    /// Delete the grapheme cluster immediately after the cursor (i.e.
    /// the Delete key).
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.content.len() {
            return;
        }
        let next = next_grapheme_boundary(&self.content, self.cursor);
        self.content.drain(self.cursor..next);
    }

    /// Move cursor one grapheme cluster to the left.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = previous_grapheme_boundary(&self.content, self.cursor);
        }
    }

    /// Move cursor one grapheme cluster to the right.
    pub fn cursor_right(&mut self) {
        if self.cursor < self.content.len() {
            self.cursor = next_grapheme_boundary(&self.content, self.cursor);
        }
    }

    /// Move cursor to the start of the previous word (whitespace-
    /// delimited; `Ctrl+Left` / `Alt+b`).
    pub fn cursor_word_left(&mut self) {
        let bytes = self.content.as_bytes();
        let mut i = self.cursor;
        // Skip trailing whitespace first (when we land at a word boundary).
        while i > 0 && bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        // Then walk back over word characters.
        while i > 0 && !bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        self.cursor = i;
    }

    /// Move cursor to the end of the current word, then past leading
    /// whitespace to the next word (`Ctrl+Right` / `Alt+f` — matches
    /// readline / Emacs convention, lands at the end of "foo" then
    /// "bar" then "baz" from a starting position of `0` in
    /// `"foo bar baz"`).
    pub fn cursor_word_right(&mut self) {
        let bytes = self.content.as_bytes();
        let len = bytes.len();
        let mut i = self.cursor;
        // Skip leading whitespace first.
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        // Then walk over word characters to land at end of word.
        while i < len && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        self.cursor = i.min(len);
    }

    /// Move cursor to start.
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end.
    pub fn cursor_end(&mut self) {
        self.cursor = self.content.len();
    }

    /// Handle a key event. Returns true if the key was consumed.
    ///
    /// Recognises:
    /// - `Char(c)` — insert (honours `max_length`).
    /// - `Backspace`, `Delete` — grapheme-cluster delete.
    /// - `Left`, `Right`, `Ctrl+Left`, `Ctrl+Right`, `Home`, `End`.
    /// - `Ctrl+A` (`KeyCode::Char('a')` with Ctrl) — cursor to home
    ///   (the field is single-line so select-all is the same move).
    /// - `Ctrl+U` — clear field.
    /// - `Ctrl+K` — delete to end of field.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(c) => {
                if ctrl {
                    match c {
                        'a' | 'A' => {
                            self.cursor_home();
                            return true;
                        }
                        'u' | 'U' => {
                            self.clear();
                            return true;
                        }
                        'k' | 'K' => {
                            // Ctrl+K: delete from cursor to end.
                            self.content.truncate(self.cursor);
                            return true;
                        }
                        'c' | 'C' => {
                            // Standard copy shortcut — field has no
                            // selection so this is a no-op.
                            return true;
                        }
                        _ => return false,
                    }
                }
                self.insert_char(c)
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Delete => {
                self.delete_forward();
                true
            }
            KeyCode::Left if ctrl => {
                self.cursor_word_left();
                true
            }
            KeyCode::Right if ctrl => {
                self.cursor_word_right();
                true
            }
            KeyCode::Left => {
                self.cursor_left();
                true
            }
            KeyCode::Right => {
                self.cursor_right();
                true
            }
            KeyCode::Home => {
                self.cursor_home();
                true
            }
            KeyCode::End => {
                self.cursor_end();
                true
            }
            _ => false,
        }
    }

    /// Returns `true` if appending `extra_graphemes` more clusters
    /// would exceed the length cap. With `0` it checks whether the
    /// current length is already at the cap.
    fn exceeds_cap_by(&self, extra_graphemes: usize) -> bool {
        match self.max_length {
            None => false,
            Some(cap) => self.grapheme_len() + extra_graphemes > cap,
        }
    }
}

/// Find the byte index of the start of the grapheme cluster that
/// ends at or before `from`. If `from == 0`, returns `0`. The result
/// always lies on a cluster boundary.
fn previous_grapheme_boundary(s: &str, from: usize) -> usize {
    if from == 0 {
        return 0;
    }
    let safe = from.min(s.len());
    // Walk clusters before `safe` and take the start of the last one.
    s[..safe]
        .grapheme_indices(true)
        .last()
        .map(|(start, _)| start)
        .unwrap_or(0)
}

/// Find the byte index of the start of the grapheme cluster that
/// **follows** the cluster beginning at or before `from`. If `from`
/// already sits at the last cluster's start, this returns the byte
/// index past the last cluster (`s.len()`).
fn next_grapheme_boundary(s: &str, from: usize) -> usize {
    if from >= s.len() {
        return s.len();
    }
    s.grapheme_indices(true)
        .find(|(start, _)| *start > from)
        .map(|(start, _)| start)
        .unwrap_or(s.len())
}

// ── Multi-line textarea input (used by the agent chat) ─────────

/// Wrapper around `ratatui_textarea::TextArea` for the chat input area.
///
/// Uses `TextArea<'static>` so the struct carries no lifetime and can be
/// stored directly in state (e.g. inside `AgentTabState`).
#[derive(Debug)]
pub struct InputArea {
    textarea: TextArea<'static>,
    /// Maximum grapheme clusters accepted. `None` = no cap. Defaults to
    /// [`InputArea::DEFAULT_MAX_LENGTH`] so an accidental 1 MB paste
    /// cannot produce a 1 MB outbound message.
    max_length: Option<usize>,
}

impl InputArea {
    /// Default length cap on the chat input — 16 KiB of user
    /// content is generous for a chat message and prevents runaway
    /// pastes without affecting normal use.
    pub const DEFAULT_MAX_LENGTH: usize = 16_384;

    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text("Type a message...");
        textarea.set_placeholder_style(Style::default().fg(Color::DarkGray));
        // Soft-wrap long lines at word boundaries so the composer grows in
        // height instead of scrolling horizontally. This lets `display_height`
        // predict how many terminal rows the buffer will occupy.
        textarea.set_wrap_mode(WrapMode::Word);
        // No block set — the InputWidget renders its own borderless prompt.
        Self {
            textarea,
            max_length: Some(Self::DEFAULT_MAX_LENGTH),
        }
    }

    /// Maximum number of input rows the layout will allocate before the
    /// textarea scrolls internally instead of growing further. Keeps a
    /// long paste from consuming the whole chat viewport.
    pub const MAX_INPUT_ROWS: u16 = 10;

    /// Number of terminal rows the current buffer occupies when wrapped to
    /// `width` columns. Used by the parent layout to size the input area
    /// dynamically: the box grows as the user types and caps at
    /// [`MAX_INPUT_ROWS`]. `width` is the *text* width (after any prompt /
    /// gutter), so the caller subtracts the prompt gutter first.
    pub fn display_height(&self, width: u16) -> u16 {
        let wrap_width = width.max(1) as usize;
        let mut rows = 0usize;
        for line in self.textarea.lines() {
            let w = UnicodeWidthStr::width(line.as_str());
            // ceil(w / wrap_width); an empty line still takes one row.
            let r = w.div_ceil(wrap_width);
            rows += r.max(1);
        }
        let rows = rows.max(1) as u16;
        rows.min(Self::MAX_INPUT_ROWS)
    }

    /// Insert a newline (Shift+Enter / Alt+Enter). Respects `max_length`.
    pub fn insert_newline(&mut self) {
        if let Some(cap) = self.max_length {
            if self.grapheme_len() >= cap {
                return;
            }
        }
        self.textarea.insert_newline();
    }

    /// Get the current text content as a single string.
    pub fn value(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Check if the textarea is empty.
    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    /// Clear all content.
    pub fn clear(&mut self) {
        self.textarea.clear();
    }

    /// Set or replace the maximum-length cap (grapheme clusters,
    /// not bytes). `None` disables the cap.
    pub fn set_max_length(&mut self, cap: Option<usize>) {
        self.max_length = cap;
    }

    pub fn max_length(&self) -> Option<usize> {
        self.max_length
    }

    /// Total grapheme clusters across all lines of the textarea.
    /// Computed lazily; `O(n)` over the buffer length.
    pub fn grapheme_len(&self) -> usize {
        self.value().graphemes(true).count()
    }

    /// Handle a key event. Returns true if the text content was modified.
    /// Enter and Escape are intercepted (not forwarded to the textarea) since
    /// the caller handles them for message sending / mode switching.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Reject keystrokes whose combined effect would breach the cap.
        // We don't know grapheme count for a keystroke a priori, so we
        // pre-flight by checking the cap (cheap; we already track
        // grapheme_len on demand).
        if let Some(cap) = self.max_length {
            if matches!(
                key.code,
                KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
            ) && self.grapheme_len() >= cap
            {
                // Already at the cap for character-affecting keys.
                // Backspace/Delete are still allowed so the user can
                // shrink before retyping.
                if matches!(key.code, KeyCode::Char(_)) {
                    return false;
                }
            }
        }
        match key.code {
            KeyCode::Enter | KeyCode::Esc => false,
            _ => self.textarea.input(key),
        }
    }

    /// Insert a paste string at the cursor position, truncated to
    /// honour `max_length`. Returns `true` if anything was inserted.
    pub fn insert_str(&mut self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        let to_insert: String = match self.max_length {
            None => s.to_string(),
            Some(cap) => {
                let current = self.grapheme_len();
                let budget = cap.saturating_sub(current);
                if budget == 0 {
                    return false;
                }
                s.graphemes(true).take(budget).collect()
            }
        };
        if to_insert.is_empty() {
            return false;
        }
        self.textarea.insert_str(&to_insert)
    }

    /// Set placeholder text (used for disabled / running state).
    pub fn set_placeholder(&mut self, text: &str) {
        self.textarea.set_placeholder_text(text);
    }

    /// Borrow the inner textarea for rendering.
    pub fn textarea(&self) -> &TextArea<'_> {
        &self.textarea
    }
}

impl Default for InputArea {
    fn default() -> Self {
        Self::new()
    }
}

/// State for [`InputWidget`].
pub struct InputWidgetState<'a> {
    pub input: &'a mut InputArea,
}

/// Width of the borderless prompt gutter ("› " = prompt glyph + space).
pub const PROMPT_GUTTER: u16 = 2;

/// Composite widget: borderless input area with a `›` prompt, backed by
/// ratatui-textarea. The textarea renders into the area to the right of the
/// prompt gutter, so its cursor and word-wrapping line up beneath the prompt.
///
/// Modeled after codex's composer: no border, a single bold prompt glyph,
/// and the textarea fills the remaining width. The parent layout passes a
/// height computed from [`InputArea::display_height`].
pub struct InputWidget<'a> {
    pub disabled: bool,
    pub title: &'a str,
    pub placeholder: &'a str,
}

impl<'a> StatefulWidget for InputWidget<'a> {
    type State = InputWidgetState<'a>;

    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer, state: &mut Self::State) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        // Render the `›` prompt at the top-left, dimmed when disabled.
        let prompt_style = if self.disabled {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };
        buf.set_string(area.x, area.y, "›", prompt_style);

        // Leave a 2-column gutter (prompt glyph + space) for the textarea.
        let inner = if area.width > PROMPT_GUTTER {
            Rect {
                x: area.x + PROMPT_GUTTER,
                width: area.width - PROMPT_GUTTER,
                ..area
            }
        } else {
            area
        };

        state.input.set_placeholder(self.placeholder);
        state.input.textarea().render(inner, buf);

        // `title` is surfaced by the parent's footer hint line; keep the
        // field so callers can still annotate the widget without a render.
        let _ = self.title;
    }
}

// ── Single-line input widget (used by the config tab) ───────────

/// State for [`SingleLineInput`]: borrows the underlying
/// [`InputState`] mutably so the widget can render its current
/// `cursor()` position.
pub struct SingleLineInputState<'a> {
    pub input: &'a mut InputState,
}

/// Single-line bordered input that renders its own cursor glyph.
///
/// Visual style mirrors [`InputWidget`]: rounded bordered block with
/// an inline title. When the field is focused the border switches to
/// `Color::Yellow`; when unfocused it dims to `Color::DarkGray`. The
/// text glyph used for the cursor is `▏` (a thin vertical bar) so
/// the cursor reads as a caret-position indicator without expanding
/// the line height.
pub struct SingleLineInput<'a> {
    pub title: &'a str,
    pub focused: bool,
    pub placeholder: &'a str,
}

impl<'a> StatefulWidget for SingleLineInput<'a> {
    type State = SingleLineInputState<'a>;

    fn render(self, area: Rect, buf: &mut ratatui::prelude::Buffer, state: &mut Self::State) {
        let border_style = if self.focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let block = Block::bordered()
            .title(format!(" {} ", self.title))
            .border_style(border_style);
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let value = state.input.value();
        let cursor = state.input.cursor();

        if value.is_empty() {
            // Empty + focused: show placeholder with a visible cursor at the
            // start of the editable area. Empty + unfocused: hint glyph.
            if self.focused {
                let placeholder_style = Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(ratatui::style::Modifier::ITALIC);
                let cursor_style = Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(ratatui::style::Modifier::SLOW_BLINK);
                let placeholder = truncate_to_width(self.placeholder, inner.width as usize);
                let placeholder_w = unicode_width::UnicodeWidthStr::width(placeholder.as_str());
                let cursor_col = inner.x;
                let placeholder_col = cursor_col + 1;
                buf.set_string(cursor_col, inner.y, "▏", cursor_style);
                if placeholder_w > 0 && placeholder_col < inner.x + inner.width {
                    let available = (inner.x + inner.width).saturating_sub(placeholder_col);
                    let chunk = truncate_to_width(&placeholder, available as usize);
                    buf.set_string(placeholder_col, inner.y, &chunk, placeholder_style);
                }
            } else {
                let hint_style = Style::default().fg(Color::DarkGray);
                let hint = truncate_to_width(self.placeholder, inner.width as usize);
                buf.set_string(inner.x, inner.y, &hint, hint_style);
            }
            return;
        }

        // Split content into before-cursor / after-cursor spans at the
        // grapheme-cluster boundary maintained by InputState. Render the
        // cursor glyph with Yellow FG, then continue with the post-cursor
        // text. Width-budgeting trims long content to the inner width.
        let before = &value[..cursor.min(value.len())];
        let after = &value[cursor.min(value.len())..];

        let inner_w = inner.width as usize;
        let cursor_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(ratatui::style::Modifier::SLOW_BLINK);
        let value_style = Style::default().fg(Color::White);
        let value_dim_style = Style::default().fg(Color::DarkGray);

        if !self.focused {
            // Unfocused: just render value plainly, no cursor.
            let trimmed = truncate_to_width(value, inner_w);
            buf.set_string(inner.x, inner.y, &trimmed, value_style);
            return;
        }

        // Focused: budget the before-segment (it pushes the cursor
        // further right when overflowing; we let the cursor glyph ride
        // off-screen rather than split content, matching how `chat`
        // widgets typically align).
        let before_w = unicode_width::UnicodeWidthStr::width(before);
        let (visible_before, cursor_x) = if before_w + 1 > inner_w {
            // Past content is wider than the editable area — keep the
            // tail just before the cursor, and place the cursor at the
            // last column.
            let drop_bytes = byte_offset_after_width(before, before_w + 1 - inner_w);
            (Some(&before[drop_bytes..]), inner.x + inner.width - 1)
        } else {
            (None, inner.x + before_w as u16)
        };
        let after_w_budget = inner_w.saturating_sub((cursor_x - inner.x) as usize + 1);
        let visible_after = truncate_to_width(after, after_w_budget);

        if let Some(visible) = visible_before {
            buf.set_string(inner.x, inner.y, visible, value_dim_style);
        }
        buf.set_string(cursor_x, inner.y, "▏", cursor_style);
        let after_x = cursor_x + 1;
        if !visible_after.is_empty() && after_x < inner.x + inner.width {
            buf.set_string(after_x, inner.y, &visible_after, value_style);
        }
        let _ = value_dim_style;
    }
}

/// Truncate `s` so that its rendered display width does not exceed
/// `max_width` columns. Avoids splitting mid-grapheme.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut width = 0;
    for grapheme in s.graphemes(true) {
        let w = unicode_width::UnicodeWidthStr::width(grapheme);
        if width + w > max_width {
            break;
        }
        out.push_str(grapheme);
        width += w;
    }
    out
}

/// Inverse of `truncate_to_width`: starting from index 0 of `s`, skip
/// `width_skip` columns of grapheme clusters and return the byte
/// offset to resume at. Used to drop content that's scrolled off the
/// left edge of the editable area.
fn byte_offset_after_width(s: &str, width_skip: usize) -> usize {
    if width_skip == 0 {
        return 0;
    }
    let mut width = 0;
    for (idx, grapheme) in s.grapheme_indices(true) {
        let w = unicode_width::UnicodeWidthStr::width(grapheme);
        if width + w > width_skip {
            return idx;
        }
        width += w;
        if width >= width_skip {
            // Account for the cluster that completes the column budget.
            return idx + grapheme.len();
        }
    }
    s.len()
}

// ── Input-history helpers (chat input Up/Down recall) ───────────

/// Append a freshly-submitted prompt to `history`, evicting the
/// oldest entry when capacity is reached. Whitespace-only entries
/// are skipped.
pub(crate) fn history_push(
    history: &mut std::collections::VecDeque<String>,
    text: String,
    capacity: usize,
) {
    if text.trim().is_empty() {
        return;
    }
    if history.len() >= capacity {
        history.pop_front();
    }
    history.push_back(text);
}

/// Handle `Up` on the chat input. Returns `true` if the key was
/// consumed (i.e. it changed the visible buffer or harmlessly
/// failed against an empty history).
///
/// State machine:
///
/// - `recall == None` and history non-empty → save current buffer
///   as `draft`, replace buffer with `history[last]`,
///   `recall = Some(last)`.
/// - `recall == Some(idx)` and `idx + 1 < len` → `recall = Some(idx+1)`,
///   replace buffer with `history[idx+1]`.
/// - `recall == Some(idx)` and `idx + 1 == len` → no-op (already at
///   the newest entry).
pub(crate) fn history_up(
    input: &mut InputArea,
    history: &std::collections::VecDeque<String>,
    draft: &mut Option<String>,
    recall: &mut Option<usize>,
) -> bool {
    if history.is_empty() {
        return false;
    }
    if let Some(idx) = *recall {
        if idx + 1 < history.len() {
            let new_idx = idx + 1;
            input.clear();
            input.insert_str(&history[new_idx]);
            *recall = Some(new_idx);
            return true;
        }
        return false;
    }
    // First Up: snapshot draft and load the newest history entry.
    *draft = Some(input.value());
    input.clear();
    let last = history.len() - 1;
    input.insert_str(&history[last]);
    *recall = Some(last);
    true
}

/// Handle `Down` on the chat input. Returns `true` if the key was
/// consumed.
///
/// State machine:
///
/// - `recall == None` → no-op (already editing the draft).
/// - `recall == Some(0)` → restore `draft`, clear recall. If the
///   draft was empty, the buffer ends up empty too.
/// - `recall == Some(idx)` with `idx > 0` → `recall = Some(idx-1)`,
///   replace buffer with `history[idx-1]`.
pub(crate) fn history_down(
    input: &mut InputArea,
    history: &std::collections::VecDeque<String>,
    draft: &mut Option<String>,
    recall: &mut Option<usize>,
) -> bool {
    let Some(idx) = *recall else {
        return false;
    };
    if idx == 0 {
        // Restore the draft buffer.
        let restored = draft.take().unwrap_or_default();
        input.clear();
        input.insert_str(&restored);
        *recall = None;
        return true;
    }
    let new_idx = idx - 1;
    input.clear();
    input.insert_str(&history[new_idx]);
    *recall = Some(new_idx);
    true
}

/// Drop any active recall state. Called when the user submits a
/// message or starts editing the buffer (any non-`Up`/`Down` key
/// after recall implies they're done browsing history).
pub(crate) fn history_clear_recall(draft: &mut Option<String>, recall: &mut Option<usize>) {
    draft.take();
    *recall = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind};
    use std::collections::VecDeque;

    fn ctrl_kc(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    /// Backspace must remove a combining-mark sequence in a single
    /// press rather than leaving an orphaned combining accent.
    #[test]
    fn grapheme_backspace_keeps_combining_clean() {
        let mut s = InputState::new();
        // `é` decomposed as `e` + combining acute `\u{0301}`.
        s.insert_char('e');
        s.insert_char('\u{0301}');
        // The two-char sequence is one grapheme cluster.
        assert_eq!(s.grapheme_len(), 1);
        // Forward / backward cursor movement must walk a single cluster.
        s.cursor_left();
        assert_eq!(
            s.cursor(),
            0,
            "one backspace of code-point over the cluster"
        );

        // Pressing Backspace must delete the whole cluster at once.
        s.cursor_end();
        s.backspace();
        assert_eq!(
            s.value(),
            "",
            "combining cluster should be deleted as a unit"
        );
        assert_eq!(s.cursor(), 0);
    }

    /// Cursor navigation should jump by graphemes, not bytes —
    /// verifies multi-byte UTF-8 navigation aligns with user
    /// expectations.
    #[test]
    fn cursor_left_walks_graphemes_not_bytes() {
        let mut s = InputState::new();
        s.insert_str("中文"); // 6 bytes (2 chars × 3 bytes each)
        assert_eq!(s.value().len(), 6);
        assert_eq!(s.grapheme_len(), 2);

        s.cursor_left();
        assert_eq!(
            s.cursor(),
            3,
            "should be after first grapheme, not middle of byte 2"
        );
        s.cursor_left();
        assert_eq!(s.cursor(), 0);
    }

    /// `insert_str` must respect `max_length`, truncating to fit.
    #[test]
    fn paste_respects_max_length() {
        let mut s = InputState::new();
        s.set_max_length(5);
        assert!(s.insert_str("hello world"));
        assert_eq!(s.value(), "hello");
        assert_eq!(s.grapheme_len(), 5);

        // Pasted content at exactly the cap should still insert
        // nothing (budget == 0).
        s.clear();
        s.set_max_length(3);
        s.insert_str("abc");
        assert!(!s.insert_str("d"), "no budget remaining");
        assert_eq!(s.value(), "abc");
    }

    /// `Ctrl+Left` / `Ctrl+Right` should jump over word tokens.
    #[test]
    fn ctrl_left_jumps_word() {
        let mut s = InputState::new();
        s.insert_str("foo bar baz");
        // Cursor is at the end after insertion.
        assert!(s.handle_key(ctrl_kc('a')));
        assert_eq!(s.cursor(), 0, "Ctrl+A → home");

        assert!(s.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL,)));
        // After Ctrl+Right from position 0, cursor lands at end of "foo".
        assert_eq!(s.cursor(), 3, "after 'foo'");
        assert!(s.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL,)));
        assert_eq!(s.cursor(), 7, "after 'foo bar'");
        assert!(s.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL,)));
        assert_eq!(s.cursor(), 4, "back to start of 'bar'");
    }

    /// `history_push` skips empty inputs and evicts oldest when full.
    #[test]
    fn history_push_skips_empty_and_caps() {
        let mut h: VecDeque<String> = VecDeque::new();
        history_push(&mut h, "first".into(), 3);
        history_push(&mut h, "   ".into(), 3); // skipped (trim is empty)
        history_push(&mut h, "second".into(), 3);
        history_push(&mut h, "third".into(), 3);
        history_push(&mut h, "fourth".into(), 3); // evicts "first"
        let collected: Vec<&str> = h.iter().map(String::as_str).collect();
        assert_eq!(collected, vec!["second", "third", "fourth"]);
    }

    /// End-to-end Up/Down state machine against a populated history.
    /// Verifies the readline convention:
    ///   - First Up snapshots the current draft and shows the newest entry.
    ///   - Subsequent Up walks toward older entries; at the oldest,
    ///     Up is a no-op.
    ///   - Down walks toward newer entries; only when already at the
    ///     oldest (`idx == 0`) does Down restore the draft and exit
    ///     recall.
    #[test]
    fn history_up_down_state_machine() {
        let mut h: VecDeque<String> = VecDeque::new();
        history_push(&mut h, "alpha".into(), 10);
        history_push(&mut h, "beta".into(), 10);
        history_push(&mut h, "gamma".into(), 10);

        let mut input = InputArea::new();
        input.insert_str("draft-msg");
        let mut draft: Option<String> = None;
        let mut recall: Option<usize> = None;

        // First Up: snapshot the working draft, show "gamma" (newest).
        assert!(history_up(&mut input, &h, &mut draft, &mut recall));
        assert_eq!(input.value(), "gamma");
        assert_eq!(recall, Some(2));
        assert_eq!(draft.as_deref(), Some("draft-msg"));

        // Up at idx == len-1 is a no-op (already at newest).
        assert!(!history_up(&mut input, &h, &mut draft, &mut recall));
        assert_eq!(input.value(), "gamma");

        // Down walks toward older: Some(2) -> Some(1) = "beta".
        assert!(history_down(&mut input, &h, &mut draft, &mut recall));
        assert_eq!(input.value(), "beta");
        assert_eq!(recall, Some(1));

        // Down again: Some(1) -> Some(0) = "alpha".
        assert!(history_down(&mut input, &h, &mut draft, &mut recall));
        assert_eq!(input.value(), "alpha");
        assert_eq!(recall, Some(0));

        // Down from Some(0) restores the draft, clears recall.
        assert!(history_down(&mut input, &h, &mut draft, &mut recall));
        assert_eq!(input.value(), "draft-msg");
        assert_eq!(recall, None);
        assert!(draft.is_none(), "draft consumed on restoration");

        // From the restored draft, Up again goes to the newest history
        // entry (we don't remember the lower idx).
        assert!(history_up(&mut input, &h, &mut draft, &mut recall));
        assert_eq!(input.value(), "gamma");
        assert_eq!(recall, Some(2));

        // Out-of-bounds up at the start: history_up on a fresh input
        // with empty history must return false and not touch state.
        let empty_h: VecDeque<String> = VecDeque::new();
        let mut input3 = InputArea::new();
        let mut draft3 = None;
        let mut recall3 = None;
        assert!(!history_up(
            &mut input3,
            &empty_h,
            &mut draft3,
            &mut recall3
        ));
        assert_eq!(recall3, None);
    }

    /// `history_down` with no recall in progress is a no-op (we're
    /// editing the draft already).
    #[test]
    fn history_down_no_recall_is_noop() {
        let mut input = InputArea::new();
        let mut h: VecDeque<String> = VecDeque::new();
        history_push(&mut h, "x".into(), 10);
        let mut draft = None;
        let mut recall = None;
        assert!(!history_down(&mut input, &h, &mut draft, &mut recall));
        assert_eq!(input.value(), "");
    }

    /// `insert_str` on `InputArea` must honour the chat-box
    /// `max_length` cap (default 16 384 chars per
    /// [`InputArea::DEFAULT_MAX_LENGTH`]).
    #[test]
    fn input_area_paste_truncates_at_cap() {
        let mut input = InputArea::new();
        // Override the default cap so the test is fast.
        input.set_max_length(Some(8));
        assert!(input.insert_str("abcdefghij"));
        // Only "abcdefgh" should fit.
        assert_eq!(input.value(), "abcdefgh");
        assert_eq!(input.grapheme_len(), 8);
        // Further pastes are silently dropped.
        assert!(!input.insert_str("xyz"));
        assert_eq!(input.value(), "abcdefgh");
    }

    /// `Ctrl+U` clears the field, `Ctrl+K` deletes from cursor to
    /// end (kill). Note: `Ctrl+K` over a single-char buffer should
    /// clear the field entirely; over an empty buffer should be a
    /// no-op (returning true so the keystroke is consumed but with
    /// no observable change).
    #[test]
    fn ctrl_u_clears_and_ctrl_k_kills() {
        let mut s = InputState::new();
        s.insert_str("hello world");
        s.cursor_home();
        for _ in 0..5 {
            s.cursor_right();
        }
        assert_eq!(s.cursor(), 5, "cursor positioned at end of 'hello'");
        assert_eq!(s.value(), "hello world");

        // Ctrl+K from position 5 deletes " world", leaving "hello".
        assert!(s.handle_key(ctrl_kc('k')));
        assert_eq!(s.value(), "hello");
        assert_eq!(s.cursor(), 5);

        // Re-fill and Ctrl+U clears the whole field.
        s.insert_str(" world");
        assert_eq!(s.value(), "hello world");
        assert!(s.handle_key(ctrl_kc('u')));
        assert_eq!(s.value(), "");
        assert_eq!(s.cursor(), 0);
    }

    // ── display_height test ───────────────

    /// `display_height` grows with the buffer and is capped at
    /// [`InputArea::MAX_INPUT_ROWS`]. A short single-line buffer is one row;
    /// a buffer wider than the wrap width wraps onto extra rows.
    #[test]
    fn display_height_grows_and_caps() {
        let mut input = InputArea::new();

        // Empty buffer → one row.
        assert_eq!(input.display_height(40), 1);

        // "hello world" is 11 columns; at width 40 it fits on one row.
        input.insert_str("hello world");
        assert_eq!(input.display_height(40), 1);

        // Force a wrap: at width 5, 11 columns wrap to ceil(11/5)=3 rows.
        assert_eq!(input.display_height(5), 3);

        // An explicit newline always adds a row.
        input.insert_newline();
        assert_eq!(input.display_height(40), 2);

        // Many lines cap at MAX_INPUT_ROWS even when the raw count is larger.
        for _ in 0..50 {
            input.insert_newline();
        }
        assert_eq!(input.display_height(40), InputArea::MAX_INPUT_ROWS);
    }
}
