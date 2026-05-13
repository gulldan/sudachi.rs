/*
 * Copyright (c) 2021-2024 Works Applications Co., Ltd.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::cmp::Ordering;

use crate::dic::lexicon_set::LexiconSet;
use crate::prelude::*;

/// A checker for words that cross boundaries
pub struct NonBreakChecker<'a> {
    lexicon: &'a LexiconSet<'a>,
    pub bos: usize,
}
impl<'a> NonBreakChecker<'a> {
    pub fn new(lexicon: &'a LexiconSet<'a>) -> Self {
        NonBreakChecker { lexicon, bos: 0 }
    }
}

impl NonBreakChecker<'_> {
    /// Returns whether there is a word that crosses the boundary

    fn has_non_break_word(&self, input: &str, length: usize) -> bool {
        // assume that SentenceDetector::get_eos called with self.input[self.bos..]
        let eos_byte = self.bos + length;
        let input_bytes = input.as_bytes();
        const LOOKUP_BYTE_LENGTH: usize = 10 * 3; // 10 Japanese characters in UTF-8
        let lookup_start = std::cmp::max(LOOKUP_BYTE_LENGTH, eos_byte) - LOOKUP_BYTE_LENGTH;
        for i in lookup_start..eos_byte {
            for entry in self.lexicon.lookup(input_bytes, i) {
                let end_byte = entry.end;
                // handling cases like モーニング娘。
                match end_byte.cmp(&eos_byte) {
                    // end is after than boundary candidate, this boundary is bad
                    Ordering::Greater => return true,
                    // end is on boundary candidate,
                    // check that there are more than one character in the matched word
                    Ordering::Equal => return input[i..].chars().take(2).count() > 1,
                    _ => {}
                }
            }
        }
        false
    }
}

const PERIOD_CHARS: &str = "。？！♪…?!";
const DOT_CHARS: &str = ".．";
const COMMA_CHARS: &str = ",，、";
const OPEN_PARENTHESIS_CHARS: &str = "({｛[（「【『［≪〔“";
const CLOSE_PARENTHESIS_CHARS: &str = ")}]）」｝】』］〕≫”";

const DEFAULT_LIMIT: usize = 4096;

/// A sentence boundary detector
pub struct SentenceDetector {
    // The maximum number of characters processed at once
    limit: usize,
}

impl Default for SentenceDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl SentenceDetector {
    pub fn new() -> Self {
        SentenceDetector {
            limit: DEFAULT_LIMIT,
        }
    }
    pub fn with_limit(limit: usize) -> Self {
        SentenceDetector { limit }
    }

    /// Returns the byte index of the detected end of the sentence.
    ///
    /// If NonBreakChecker is given, it is used to determine if there is a
    /// word that crosses the detected boundary, and if so, the next boundary is
    /// returned.
    ///
    /// If there is no boundary, this returns a relatively harmles boundary as a
    /// negative value.
    ///
    /// # Examples
    ///
    /// ```
    /// let sd = sudachi::sentence_detector::SentenceDetector::new();
    /// assert_eq!(12, sd.get_eos("あいう。えお", None).unwrap());
    /// assert_eq!(-15, sd.get_eos("あいうえお", None).unwrap());
    /// ```
    pub fn get_eos(&self, input: &str, checker: Option<&NonBreakChecker>) -> SudachiResult<isize> {
        if input.is_empty() {
            return Ok(0);
        }

        // Handle at most self.limit chars at once without allocating a String.
        let (s, input_exceeds_limit) = limited_slice(input, self.limit);
        let mut parenthesis_level = 0usize;
        let mut index = 0usize;

        while index < s.len() {
            let c = s[index..].chars().next().unwrap();
            let char_end = index + c.len_utf8();

            if is_open_parenthesis(c) {
                parenthesis_level += 1;
                index = char_end;
                continue;
            }
            if is_close_parenthesis(c) {
                parenthesis_level = parenthesis_level.saturating_sub(1);
                index = char_end;
                continue;
            }

            let Some(match_end) = sentence_break_end(s, index, c, char_end) else {
                index = char_end;
                continue;
            };

            if parenthesis_level == 0 {
                let mut eos = match_end;
                if eos < s.len() {
                    eos += prohibited_bos_len(&s[eos..]);
                }
                if !is_itemize_header(s)
                    && (eos == s.len() || !is_continuous_phrase(s, eos))
                    && checker
                        .map(|ck| !ck.has_non_break_word(input, eos))
                        .unwrap_or(true)
                {
                    return Ok(eos as isize);
                }
            }

            // Match fancy_regex::find_iter behavior: rejected matches resume
            // scanning after the regex match, not after any post-match checks.
            index = match_end;
        }

        if input_exceeds_limit {
            // Search the final whitespace as a provisional split.
            if let Some(end) = final_whitespace_end(s) {
                return Ok(-(end as isize));
            }
        }

        Ok(-(s.len() as isize))
    }
}

fn limited_slice(input: &str, limit: usize) -> (&str, bool) {
    let mut iter = input.char_indices();
    for _ in 0..limit {
        if iter.next().is_none() {
            return (input, false);
        }
    }

    match iter.next() {
        Some((idx, _)) => (&input[..idx], true),
        None => (input, false),
    }
}

fn sentence_break_end(s: &str, index: usize, c: char, char_end: usize) -> Option<usize> {
    if let Some(end) = br_tag_break_end(s, index) {
        return Some(end);
    }

    let end = if is_period(c) {
        Some(char_end)
    } else if c == '・' {
        cdots_end(s, char_end)
    } else if is_dot(c) && !previous_is_alphabet_or_number(s, index) {
        match s[char_end..].chars().next() {
            Some(next) if is_alphabet_or_number(next) || is_comma(next) => None,
            _ => Some(char_end),
        }
    } else {
        None
    }?;

    Some(consume_dot_periods(s, end))
}

fn br_tag_break_end(s: &str, index: usize) -> Option<usize> {
    let mut end = index;
    let mut count = 0usize;
    loop {
        let rest = &s[end..];
        if rest.starts_with("<br>") || rest.starts_with("<BR>") {
            end += 4;
            count += 1;
        } else {
            break;
        }
    }

    (count >= 2).then_some(end)
}

fn cdots_end(s: &str, char_end: usize) -> Option<usize> {
    let mut end = char_end;
    let mut count = 1usize;
    while let Some(c) = s[end..].chars().next() {
        if c != '・' {
            break;
        }
        end += c.len_utf8();
        count += 1;
    }

    (count >= 3).then_some(end)
}

fn consume_dot_periods(s: &str, mut end: usize) -> usize {
    while let Some(c) = s[end..].chars().next() {
        if !is_dot(c) && !is_period(c) {
            break;
        }
        end += c.len_utf8();
    }
    end
}

/// Returns a byte length of chars at the beginning of str, which cannot be a bos.
fn prohibited_bos_len(s: &str) -> usize {
    let mut len = 0usize;
    for c in s.chars() {
        if is_close_parenthesis(c) || is_comma(c) || is_period(c) {
            len += c.len_utf8();
        } else {
            break;
        }
    }
    len
}

// Returns if eos is the middle of phrase.
fn is_continuous_phrase(s: &str, eos: usize) -> bool {
    // we can safely unwrap since eos > 0
    let (last_char_start, _) = s[..eos].char_indices().next_back().unwrap();
    if starts_with_quote_marker(&s[last_char_start..]) {
        return true;
    }

    // we can safely unwrap since eos < s.len()
    let c = s[eos..].chars().next().unwrap();
    (c == 'と' || c == 'や' || c == 'の') && ends_with_itemize_header(&s[..eos])
}

fn starts_with_quote_marker(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, '！' | '？' | '!' | '?') && !is_close_parenthesis(first) {
        return false;
    }

    let rest = chars.as_str();
    rest.starts_with('と') || rest.starts_with('っ') || rest.starts_with("です")
}

fn is_itemize_header(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let Some(second) = chars.next() else {
        return false;
    };
    chars.next().is_none() && is_alphabet_or_number(first) && is_dot(second)
}

fn ends_with_itemize_header(s: &str) -> bool {
    let mut chars = s.chars().rev();
    let Some(last) = chars.next() else {
        return false;
    };
    let Some(prev) = chars.next() else {
        return false;
    };
    is_dot(last) && is_alphabet_or_number(prev)
}

fn previous_is_alphabet_or_number(s: &str, index: usize) -> bool {
    s[..index]
        .chars()
        .next_back()
        .map(is_alphabet_or_number)
        .unwrap_or(false)
}

fn final_whitespace_end(s: &str) -> Option<usize> {
    let mut last = None;
    for (idx, c) in s.char_indices() {
        if idx > 0 && c.is_whitespace() {
            last = Some(idx + c.len_utf8());
        }
    }
    last
}

fn is_period(c: char) -> bool {
    PERIOD_CHARS.contains(c)
}

fn is_dot(c: char) -> bool {
    DOT_CHARS.contains(c)
}

fn is_comma(c: char) -> bool {
    COMMA_CHARS.contains(c)
}

fn is_open_parenthesis(c: char) -> bool {
    OPEN_PARENTHESIS_CHARS.contains(c)
}

fn is_close_parenthesis(c: char) -> bool {
    CLOSE_PARENTHESIS_CHARS.contains(c)
}

fn is_alphabet_or_number(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || ('ａ'..='ｚ').contains(&c)
        || ('Ａ'..='Ｚ').contains(&c)
        || ('０'..='９').contains(&c)
        || matches!(
            c,
            '〇' | '一'
                | '二'
                | '三'
                | '四'
                | '五'
                | '六'
                | '七'
                | '八'
                | '九'
                | '十'
                | '百'
                | '千'
                | '万'
                | '億'
                | '兆'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_eos() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos("あいうえお。", None).unwrap(), 18);
        assert_eq!(sd.get_eos("あいう。えお。", None).unwrap(), 12);
        assert_eq!(sd.get_eos("あいう。。えお。", None).unwrap(), 15);
        assert_eq!(sd.get_eos("あいうえお", None).unwrap(), -15);
        assert_eq!(sd.get_eos("あいう えお。", None).unwrap(), 19);
        assert_eq!(sd.get_eos("あいう えお", None).unwrap(), -16);
        assert_eq!(sd.get_eos("", None).unwrap(), 0);
    }

    #[test]
    fn get_eos_with_limit() {
        let sd = SentenceDetector::with_limit(5);
        assert_eq!(sd.get_eos("あいうえおか。", None).unwrap(), -15);
        assert_eq!(sd.get_eos("あい。うえお。", None).unwrap(), 9);
        assert_eq!(sd.get_eos("あいうえ", None).unwrap(), -12);
        assert_eq!(sd.get_eos("あい うえお", None).unwrap(), -7);
        assert_eq!(sd.get_eos("あ い うえお", None).unwrap(), -8);
    }

    #[test]
    fn get_eos_with_period() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos("あいう.えお", None).unwrap(), 10);
        assert_eq!(sd.get_eos("3.141", None).unwrap(), -5);
        assert_eq!(sd.get_eos("四百十.〇", None).unwrap(), -13);
    }

    #[test]
    fn get_eos_with_many_periods() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos("あいうえお!??", None).unwrap(), 18);
    }

    #[test]
    fn get_eos_with_br_tags_and_cdots() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos("あ<br><br>い", None).unwrap(), 11);
        assert_eq!(sd.get_eos("あ<BR><br>い", None).unwrap(), 11);
        assert_eq!(sd.get_eos("あ・・・い", None).unwrap(), 12);
        assert_eq!(sd.get_eos("あ・・い", None).unwrap(), -12);
    }

    #[test]
    fn get_eos_with_dot_boundaries() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos(".あ", None).unwrap(), 1);
        assert_eq!(sd.get_eos("A.あ", None).unwrap(), -5);
        assert_eq!(sd.get_eos("１．２", None).unwrap(), -9);
    }

    #[test]
    fn get_eos_with_parentheses() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos("あ（いう。え）お", None).unwrap(), -24);
        assert_eq!(sd.get_eos("（あ（いう）。え）お", None).unwrap(), -30);
        assert_eq!(sd.get_eos("あ（いう）。えお", None).unwrap(), 18);
    }

    #[test]
    fn get_eos_with_itemize_header() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos("1. あいう。えお", None).unwrap(), 15);
    }

    #[test]
    fn get_eos_with_prohibited_bos() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos("あいう?えお", None).unwrap(), 10);
        assert_eq!(sd.get_eos("あいう?)えお", None).unwrap(), 11);
        assert_eq!(sd.get_eos("あいう?,えお", None).unwrap(), 11);
    }

    #[test]
    fn get_eos_with_continuous_phrase() {
        let sd = SentenceDetector::new();
        assert_eq!(sd.get_eos("あいう?です。", None).unwrap(), 19);
        assert_eq!(sd.get_eos("あいう?って。", None).unwrap(), 19);
        assert_eq!(sd.get_eos("あいう?という。", None).unwrap(), 22);
        assert_eq!(sd.get_eos("あいう?の？です。", None).unwrap(), 10);

        assert_eq!(sd.get_eos("1.と2.が。", None).unwrap(), 13);
        assert_eq!(sd.get_eos("1.やb.から。", None).unwrap(), 16);
        assert_eq!(sd.get_eos("1.の12.が。", None).unwrap(), 14);
    }
}
