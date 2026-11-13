//! The Apps tab's search: what has been typed, where the caret is, what is
//! selected, and which apps it matches. Pure logic with no Win32, so every
//! rule can be tested.
//!
//! Positions count characters, not bytes, so the caret never lands inside one.
//! The selection runs from an anchor to the caret; with nothing selected, the
//! two are the same.

use crate::apps::AppRow;

/// The longest search kept, which is far longer than any app's name.
const MAX_CHARS: usize = 60;

#[derive(Debug, Default)]
pub struct Search {
    text: String,
    caret: usize,
    anchor: usize,
    /// The field has been clicked or Ctrl+F pressed, so it shows its caret
    /// even before anything is typed.
    focused: bool,
}

impl Search {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// The selected characters, as an ordered range, when there are any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        (self.anchor != self.caret)
            .then(|| (self.anchor.min(self.caret), self.anchor.max(self.caret)))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text.chars().skip(start).take(end - start).collect())
    }

    /// Whether the field is in use: focused, or holding a search.
    pub fn is_active(&self) -> bool {
        self.focused || !self.text.is_empty()
    }

    /// Whether the field has the keyboard, and so shows its caret.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn focus(&mut self) {
        self.focused = true;
    }

    /// Lets go of the field. A search that is still filtering stays.
    pub fn blur(&mut self) {
        self.focused = false;
        self.anchor = self.caret;
    }

    fn byte(&self, position: usize) -> usize {
        self.text
            .char_indices()
            .nth(position)
            .map_or(self.text.len(), |(byte, _)| byte)
    }

    /// Replaces the selection, or inserts at the caret, keeping to the limit.
    fn replace(&mut self, insert: &str) -> bool {
        let (start, end) = self.selection().unwrap_or((self.caret, self.caret));
        let room = MAX_CHARS - (self.len() - (end - start));
        let insert: String = insert.chars().take(room).collect();
        if start == end && insert.is_empty() {
            return false;
        }
        let (from, to) = (self.byte(start), self.byte(end));
        self.text.replace_range(from..to, &insert);
        self.caret = start + insert.chars().count();
        self.anchor = self.caret;
        true
    }

    /// Adds a typed character, in place of any selection. Control characters
    /// are ignored, and so is a space at the very start, which is never the
    /// start of a name. Returns whether the search changed.
    pub fn type_char(&mut self, c: char) -> bool {
        let at_start = self.selection().map_or(self.caret, |(start, _)| start) == 0;
        if c.is_control() || (c == ' ' && at_start) {
            return false;
        }
        self.focused = true;
        self.replace(&c.to_string())
    }

    /// Pastes the first line of `text`, without control characters, in place
    /// of any selection.
    pub fn paste(&mut self, text: &str) -> bool {
        let line: String = text
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        let line = if self.selection().map_or(self.caret, |(start, _)| start) == 0 {
            line.trim_start()
        } else {
            &line
        };
        self.focused = true;
        self.replace(line)
    }

    /// Removes the selection and returns it, for Ctrl+X.
    pub fn cut(&mut self) -> Option<String> {
        let taken = self.selected_text()?;
        self.replace("");
        Some(taken)
    }

    /// Deletes from `from` to the caret, or the selection if there is one.
    fn delete_to(&mut self, from: usize) -> bool {
        if self.selection().is_none() {
            self.anchor = from;
        }
        self.replace("")
    }

    /// Backspace, or Ctrl+Backspace for the whole word before the caret.
    pub fn backspace(&mut self, word: bool) -> bool {
        let from = if word {
            self.word_before(self.caret)
        } else {
            self.caret.saturating_sub(1)
        };
        self.delete_to(from)
    }

    /// Delete, or Ctrl+Delete for the whole word after the caret.
    pub fn delete(&mut self, word: bool) -> bool {
        let to = if word {
            self.word_after(self.caret)
        } else {
            (self.caret + 1).min(self.len())
        };
        self.delete_to(to)
    }

    /// Where the word before `position` starts, past any spaces.
    fn word_before(&self, position: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut at = position;
        while at > 0 && chars[at - 1].is_whitespace() {
            at -= 1;
        }
        while at > 0 && !chars[at - 1].is_whitespace() {
            at -= 1;
        }
        at
    }

    /// Where the word after `position` ends, with the spaces after it.
    fn word_after(&self, position: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut at = position;
        while at < chars.len() && !chars[at].is_whitespace() {
            at += 1;
        }
        while at < chars.len() && chars[at].is_whitespace() {
            at += 1;
        }
        at
    }

    /// Moves the caret, keeping the anchor to extend the selection with Shift.
    pub fn move_to(&mut self, position: usize, extend: bool) {
        self.caret = position.min(self.len());
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// The Left arrow. Without Shift, a selection collapses to its start.
    pub fn left(&mut self, word: bool, extend: bool) {
        match self.selection() {
            Some((start, _)) if !extend => self.move_to(start, false),
            _ if word => self.move_to(self.word_before(self.caret), extend),
            _ => self.move_to(self.caret.saturating_sub(1), extend),
        }
    }

    /// The Right arrow. Without Shift, a selection collapses to its end.
    pub fn right(&mut self, word: bool, extend: bool) {
        match self.selection() {
            Some((_, end)) if !extend => self.move_to(end, false),
            _ if word => self.move_to(self.word_after(self.caret), extend),
            _ => self.move_to(self.caret + 1, extend),
        }
    }

    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.move_to(self.len(), extend);
    }

    /// Ctrl+A.
    pub fn select_all(&mut self) {
        self.focused = true;
        self.anchor = 0;
        self.caret = self.len();
    }

    /// A double-click: selects the word at `position`.
    pub fn select_word(&mut self, position: usize) {
        let chars: Vec<char> = self.text.chars().collect();
        let position = position.min(chars.len());
        let on_word = |at: usize| chars.get(at).is_some_and(|c| !c.is_whitespace());
        let mut start = position;
        let mut end = position;
        if !on_word(position) && position > 0 && on_word(position - 1) {
            start -= 1;
            end -= 1;
        }
        while start > 0 && on_word(start - 1) {
            start -= 1;
        }
        while on_word(end) {
            end += 1;
        }
        self.focused = true;
        self.anchor = start;
        self.caret = end;
    }

    pub fn clear(&mut self) -> bool {
        let had = !self.text.is_empty();
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
        had
    }

    /// Esc: clears the search, or with nothing typed, leaves the field.
    /// Returns whether it did anything.
    pub fn escape(&mut self) -> bool {
        if self.clear() {
            return true;
        }
        std::mem::take(&mut self.focused)
    }

    /// Whether an app matches: its name, or the name of its exe.
    pub fn matches(&self, row: &AppRow) -> bool {
        let query = self.text.trim();
        query.is_empty() || find(&row.name, query).is_some() || find(&row.key, query).is_some()
    }
}

/// Where `query` first appears in `text`, ignoring case, as a range of
/// characters rather than bytes.
pub fn find(text: &str, query: &str) -> Option<(usize, usize)> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let text: Vec<char> = text.chars().map(fold).collect();
    let query: Vec<char> = query.chars().map(fold).collect();
    if query.is_empty() || query.len() > text.len() {
        return None;
    }
    (0..=text.len() - query.len())
        .find(|&start| text[start..start + query.len()] == query[..])
        .map(|start| (start, start + query.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::Usage;

    fn row(key: &str, name: &str) -> AppRow {
        AppRow {
            key: key.to_string(),
            name: name.to_string(),
            icon: None,
            usage: Usage::default(),
            frozen: false,
            can_end: true,
            processes: Vec::new(),
        }
    }

    fn typed(text: &str) -> Search {
        let mut search = Search::default();
        for c in text.chars() {
            search.type_char(c);
        }
        search
    }

    #[test]
    fn matching_ignores_case_and_finds_any_part_of_the_name() {
        let search = typed("st");
        assert!(search.matches(&row("code", "Visual Studio Code")));
        assert!(search.matches(&row("steam", "Steam")));
        assert!(!search.matches(&row("firefox", "Firefox")));
    }

    #[test]
    fn the_exe_name_matches_too() {
        assert!(typed("msedge").matches(&row("msedge", "Microsoft Edge")));
    }

    #[test]
    fn an_empty_search_matches_everything() {
        assert!(Search::default().matches(&row("dwm", "dwm")));
        assert!(typed("a ").matches(&row("dwm", "Adwm")));
    }

    #[test]
    fn find_counts_characters_not_bytes() {
        assert_eq!(find("Café Désign", "dés"), Some((5, 8)));
        assert_eq!(find("Steam", "STE"), Some((0, 3)));
        assert_eq!(find("Steam", "x"), None);
        assert_eq!(find("st", "steam"), None);
    }

    #[test]
    fn control_characters_and_a_leading_space_are_ignored() {
        let mut search = Search::default();
        assert!(!search.type_char(' '));
        assert!(!search.type_char('\u{1b}'));
        assert!(search.type_char('o'));
        assert!(search.type_char(' '));
        assert_eq!(search.text(), "o ");
    }

    #[test]
    fn paste_keeps_the_first_line() {
        let mut search = Search::default();
        assert!(search.paste("  discord\nsecond line"));
        assert_eq!(search.text(), "discord");
        assert_eq!(search.caret(), 7);
    }

    #[test]
    fn the_search_never_grows_past_its_limit() {
        let mut search = typed(&"a".repeat(100));
        assert_eq!(search.text().chars().count(), MAX_CHARS);
        assert!(!search.paste("more"));
    }

    #[test]
    fn select_all_then_typing_replaces_everything() {
        let mut search = typed("steam");
        search.select_all();
        assert_eq!(search.selection(), Some((0, 5)));
        assert_eq!(search.selected_text().as_deref(), Some("steam"));
        assert!(search.type_char('d'));
        assert_eq!(search.text(), "d");
        assert_eq!(search.selection(), None);
    }

    #[test]
    fn select_all_then_backspace_empties_the_field() {
        let mut search = typed("discord");
        search.select_all();
        assert!(search.backspace(false));
        assert_eq!(search.text(), "");
        assert_eq!(search.caret(), 0);
    }

    #[test]
    fn typing_goes_in_at_the_caret() {
        let mut search = typed("stam");
        search.left(false, false);
        search.left(false, false);
        search.type_char('e');
        assert_eq!(search.text(), "steam");
        assert_eq!(search.caret(), 3);
    }

    #[test]
    fn shift_with_the_arrows_selects_and_plain_arrows_collapse() {
        let mut search = typed("visual studio");
        search.home(false);
        search.right(true, true);
        assert_eq!(search.selected_text().as_deref(), Some("visual "));
        search.right(false, false);
        assert_eq!((search.caret(), search.selection()), (7, None));
        search.end(true);
        assert_eq!(search.selected_text().as_deref(), Some("studio"));
        search.left(false, false);
        assert_eq!(search.caret(), 7);
    }

    #[test]
    fn ctrl_backspace_and_ctrl_delete_take_whole_words() {
        let mut search = typed("visual studio  ");
        assert!(search.backspace(true));
        assert_eq!(search.text(), "visual ");
        search.home(false);
        assert!(search.delete(true));
        assert_eq!(search.text(), "");
        assert!(!search.backspace(true));
    }

    #[test]
    fn delete_removes_the_character_after_the_caret() {
        let mut search = typed("steeam");
        search.move_to(3, false);
        assert!(search.delete(false));
        assert_eq!(search.text(), "steam");
        search.end(false);
        assert!(!search.delete(false));
    }

    #[test]
    fn cut_takes_the_selection() {
        let mut search = typed("microsoft edge");
        search.select_word(2);
        assert_eq!(search.cut().as_deref(), Some("microsoft"));
        assert_eq!(search.text(), " edge");
        assert_eq!(search.cut(), None);
    }

    #[test]
    fn double_click_selects_the_word_under_the_pointer() {
        let mut search = typed("visual studio code");
        search.select_word(9);
        assert_eq!(search.selected_text().as_deref(), Some("studio"));
        search.select_word(13);
        assert_eq!(
            search.selected_text().as_deref(),
            Some("studio"),
            "just after it"
        );
    }

    #[test]
    fn pasting_replaces_the_selection() {
        let mut search = typed("steam");
        search.select_all();
        assert!(search.paste("discord"));
        assert_eq!(search.text(), "discord");
    }

    #[test]
    fn positions_count_characters() {
        let mut search = typed("café");
        search.left(false, false);
        search.type_char('x');
        assert_eq!(search.text(), "cafxé");
    }

    #[test]
    fn escape_clears_first_then_leaves_the_field() {
        let mut search = typed("st");
        assert!(search.escape());
        assert_eq!(search.text(), "");
        assert!(search.is_active(), "still focused after clearing");
        assert!(search.escape());
        assert!(!search.is_active());
        assert!(!search.escape());
    }

    #[test]
    fn blurring_keeps_a_search_that_is_filtering() {
        let mut search = typed("st");
        search.select_all();
        search.blur();
        assert!(search.is_active());
        assert_eq!(search.text(), "st");
        assert_eq!(search.selection(), None);
    }
}
