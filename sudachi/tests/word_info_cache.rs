/*
 * Copyright (c) 2026 Works Applications Co., Ltd.
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

//! Tokenizer-level behaviour of the (always-on) WordInfo cache. Its keying and
//! sharing are unit-tested in `dic::lexicon_set`; these cover the two paths that
//! reach the tokenizer rather than the cache directly: OOV (which must bypass it)
//! and user-dictionary POS resolution.

extern crate sudachi;

use sudachi::prelude::Mode;

mod common;
use crate::common::{TestStatefulTokenizer, LEX_CSV, USER1_CSV};

/// Concatenated normalized forms of the OOV morphemes produced for `surface`
/// (robust to how the OOV run is segmented).
fn oov_normalized(tok: &mut TestStatefulTokenizer, surface: &str) -> String {
    let ms = tok.tokenize(surface);
    let mut out = String::new();
    let mut saw_oov = false;
    for i in 0..ms.len() {
        let m = ms.get(i);
        if m.word_id().is_oov() {
            out.push_str(m.normalized_form());
            saw_oov = true;
        }
    }
    assert!(saw_oov, "expected at least one OOV morpheme for {surface:?}");
    out
}

/// OOV strings come from the input surface, not the dictionary, so OOV bypasses the
/// cache. Two equal-length katakana runs absent from the dict share one OOV word id;
/// if OOV were served from the (word_id-keyed) cache, the second surface would
/// inherit the first's strings.
#[test]
fn oov_strings_follow_surface_not_cache() {
    let mut tok = TestStatefulTokenizer::new_built(Mode::C);
    let first = oov_normalized(&mut tok, "ザザザ");
    let second = oov_normalized(&mut tok, "ゾゾゾ");
    assert_eq!(first, "ザザザ");
    assert_eq!(second, "ゾゾゾ");
    assert_ne!(
        first, second,
        "the second OOV surface must not inherit the first's cached strings"
    );
}

/// A user-dictionary POS lives above the system POS range and is rebased by the user
/// dict's `pos_offset` during resolution; it must resolve correctly through the cache.
#[test]
fn user_dictionary_pos_resolves_through_cache() {
    let mut tok = TestStatefulTokenizer::builder(LEX_CSV)
        .user(USER1_CSV)
        .mode(Mode::C)
        .build();
    let ms = tok.tokenize("すだち");
    assert!(!ms.is_empty());
    let pos = ms.get(0).part_of_speech().to_vec();
    assert_eq!(
        pos,
        vec![
            "被子植物門",
            "双子葉植物綱",
            "ムクロジ目",
            "ミカン科",
            "ミカン属",
            "スダチ"
        ]
    );
}
