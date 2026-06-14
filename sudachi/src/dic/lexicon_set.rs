/*
 * Copyright (c) 2021-2026 Works Applications Co., Ltd.
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

use thiserror::Error;

use crate::dic::binary_loader::BinaryLexicon;
use crate::dic::lexicon::strings::StringPointer;
use crate::dic::lexicon::{Lexicon, LexiconEntry, MAX_DICTIONARIES};
use crate::dic::subset::InfoSubset;
use crate::dic::word_id::{DictId, WordId};
use crate::dic::strings_cache::StringsCache;
use crate::dic::word_info::{WordInfo, WordInfoData, WordInfoEntryIdCursor};
use crate::dic::LexiconAccess;
use std::sync::Arc;
use crate::prelude::*;

/// Sudachi error
#[derive(Error, Debug, Eq, PartialEq)]
pub enum LexiconSetError {
    #[error("too large word_id {0} in dict {1}")]
    TooLargeWordId(u32, usize),

    #[error("too large dictionary_id {0}")]
    TooLargeDictionaryId(usize),

    #[error("too many user dictionaries")]
    TooManyDictionaries,

    #[error("invalid string pointer of length={0}, offset={1}, alignment={2}")]
    InvalidStringPointer(usize, usize, usize),
}

/// Set of Lexicons
///
/// Handles multiple lexicons as one lexicon
/// The first lexicon in the list must be from system dictionary
pub struct LexiconSet<'a> {
    lexicons: Vec<Lexicon<'a>>,
    pos_offsets: Vec<usize>,
    num_system_pos: usize,
}

#[doc(hidden)]
pub struct WordIdCursor {
    lexicon_index: usize,
    entry_cursor: Option<WordInfoEntryIdCursor>,
}

impl LexiconAccess for LexiconSet<'_> {
    fn lexicon(&self) -> &LexiconSet<'_> {
        self
    }
}

impl<'a> LexiconSet<'a> {
    /// Creates a LexiconSet from a system lexicon
    pub fn from_system_binary(
        system_lexicon: BinaryLexicon<'a>,
        num_system_pos: usize,
    ) -> LexiconSet<'a> {
        let mut lexicon = Lexicon::from_binary(system_lexicon);
        lexicon.set_dic_id(0);
        LexiconSet {
            lexicons: vec![lexicon],
            pos_offsets: vec![0],
            num_system_pos,
        }
    }

    /// Creates a LexiconSet given a system lexicon
    pub fn new(mut system_lexicon: Lexicon<'a>, num_system_pos: usize) -> LexiconSet<'a> {
        system_lexicon.set_dic_id(0);
        LexiconSet {
            lexicons: vec![system_lexicon],
            pos_offsets: vec![0],
            num_system_pos,
        }
    }

    /// Add a lexicon to the lexicon list
    ///
    /// pos_offset: number of pos in the grammar
    pub fn append(
        &mut self,
        mut lexicon: Lexicon<'a>,
        pos_offset: usize,
    ) -> Result<(), LexiconSetError> {
        if self.is_full() {
            return Err(LexiconSetError::TooManyDictionaries);
        }
        lexicon.set_dic_id(self.lexicons.len() as u8);
        self.lexicons.push(lexicon);
        self.pos_offsets.push(pos_offset);
        Ok(())
    }

    /// Returns if dictionary capacity is full
    pub fn is_full(&self) -> bool {
        self.lexicons.len() >= MAX_DICTIONARIES
    }
}

impl LexiconSet<'_> {
    /// Returns iterator which yields all words in the dictionary, starting from the `offset` bytes
    ///
    /// Searches dictionaries in the reverse order: user dictionaries first and then system dictionary
    #[inline]
    pub fn lookup<'b>(
        &'b self,
        input: &'b [u8],
        offset: usize,
    ) -> impl Iterator<Item = LexiconEntry> + 'b {
        // word_id fixup was moved to lexicon itself
        self.lexicons
            .iter()
            .rev()
            .flat_map(move |l| l.lookup(input, offset))
    }

    /// Pipelined + prefetched batch form of [`LexiconSet::lookup`] (issue #117).
    ///
    /// Calls `emit(bucket, entry)` for every match at every start, where
    /// `bucket` indexes into `starts`. For each bucket the entries are emitted
    /// in exactly the order repeated [`LexiconSet::lookup`] calls would yield
    /// (user dictionaries first, then system, each in trie-walk order), so
    /// grouping by `bucket` reproduces the scalar result. The trie walks of the
    /// independent starts are overlapped to hide memory latency.
    #[inline]
    pub fn lookup_batch<F: FnMut(usize, LexiconEntry)>(
        &self,
        input: &[u8],
        starts: &[usize],
        mut emit: F,
    ) {
        // Dictionaries are processed in the same reverse order as lookup(), and
        // sequentially, so each bucket accumulates one lexicon's matches fully
        // before the next lexicon's — matching lookup()'s flat_map order.
        for lexicon in self.lexicons.iter().rev() {
            lexicon.lookup_batch(input, starts, &mut emit);
        }
    }

    /// Checks prefix end offsets in the same dictionary order as lookup(), but
    /// without expanding trie leaves into word IDs.
    #[inline]
    pub(crate) fn check_prefix_ends<F>(
        &self,
        input: &[u8],
        offset: usize,
        mut check: F,
    ) -> Option<bool>
    where
        F: FnMut(usize) -> Option<bool>,
    {
        for lexicon in self.lexicons.iter().rev() {
            for end in lexicon.lookup_prefix_ends(input, offset) {
                if let Some(result) = check(end) {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Returns WordInfo for given WordId
    pub fn get_word_info(&self, id: WordId) -> SudachiResult<WordInfo> {
        self.get_word_info_subset(id, InfoSubset::all())
    }

    /// Resolves the [`WordInfoData`] for a word id and subset, without wrapping it
    /// in a [`WordInfo`]. The tokenizer-local `WordInfoCache` uses this so it can
    /// share the resolved data (and a fresh strings cache) behind `Arc`.
    pub(crate) fn get_word_info_data_subset(
        &self,
        id: WordId,
        subset: InfoSubset,
    ) -> SudachiResult<WordInfoData> {
        let dict_id = id.dict();
        let lexicon = self
            .lexicons
            .get(dict_id.as_raw() as usize)
            .ok_or(SudachiError::InvalidWordId(id))?;
        Ok(lexicon.get_word_info(id.entry(), subset)?.resolve(
            dict_id,
            self.num_system_pos,
            &self.pos_offsets,
            subset,
        ))
    }

    /// Returns WordInfo for given WordId.
    /// Only fills a requested subset of fields.
    /// Rest will be of default values (0 or empty).
    pub fn get_word_info_subset(&self, id: WordId, subset: InfoSubset) -> SudachiResult<WordInfo> {
        Ok(WordInfo::new(self.get_word_info_data_subset(id, subset)?, id))
    }

    /// Returns word_param for given word_id
    pub fn get_word_param(&self, id: WordId) -> (i16, i16, i16) {
        let dict_id = id.dict().as_raw() as usize;
        self.lexicons[dict_id].get_word_param(id.entry())
    }

    /// Returns word_param for given word_id.
    pub fn get_word_param_checked(&self, id: WordId) -> SudachiResult<(i16, i16, i16)> {
        let dict_id = id.dict().as_raw() as usize;
        match self.lexicons.get(dict_id) {
            Some(lexicon) => lexicon
                .get_word_param_checked(id.entry())
                .ok_or(SudachiError::InvalidWordId(id)),
            None => Err(SudachiError::InvalidWordId(id)),
        }
    }

    #[inline]
    pub fn get_string(&self, word_id: WordId, strptr: StringPointer) -> SudachiResult<String> {
        self.lexicons[word_id.dict().as_raw() as usize].get_string(strptr)
    }

    pub fn size(&self) -> u32 {
        self.lexicons.iter().fold(0, |acc, lex| acc + lex.size())
    }

    pub fn word_ids(&self) -> impl Iterator<Item = SudachiResult<WordId>> + '_ {
        self.lexicons.iter().enumerate().flat_map(|(dict_id, lex)| {
            let dict_id = DictId::new(dict_id as u8);
            lex.entry_ids()
                .map(move |entry| entry.map(|entry| WordId::from_parts(dict_id, entry)))
        })
    }

    #[doc(hidden)]
    pub fn word_id_cursor(&self) -> WordIdCursor {
        WordIdCursor {
            lexicon_index: 0,
            entry_cursor: self.lexicons.first().map(Lexicon::entry_id_cursor),
        }
    }

    #[doc(hidden)]
    pub fn next_word_id(&self, cursor: &mut WordIdCursor) -> SudachiResult<Option<WordId>> {
        loop {
            let Some(lexicon) = self.lexicons.get(cursor.lexicon_index) else {
                return Ok(None);
            };
            let Some(entry_cursor) = cursor.entry_cursor.as_mut() else {
                return Ok(None);
            };
            if let Some(entry) = lexicon.next_entry_id(entry_cursor)? {
                let dict_id = DictId::new(cursor.lexicon_index as u8);
                return Ok(Some(WordId::from_parts(dict_id, entry)));
            }

            cursor.lexicon_index += 1;
            cursor.entry_cursor = self
                .lexicons
                .get(cursor.lexicon_index)
                .map(Lexicon::entry_id_cursor);
        }
    }

    pub fn system_word_ids_in_order(&self) -> Vec<WordId> {
        if self.lexicons.is_empty() {
            return Vec::new();
        }
        self.lexicons[0]
            .entry_ids_in_order()
            .into_iter()
            .map(|entry| WordId::from_parts(DictId::SYSTEM, entry))
            .collect()
    }
}

// --- tokenizer-local WordInfo cache -------------------------------------------
// Resolving a `WordInfo` (parse + reference resolution) and decoding its strings
// repeats for every occurrence of a word. This bounded, tokenizer-local cache
// resolves each `(word_id, subset)` once and hands later occurrences out as shared
// `Arc`s. It is owned by the tokenizer, so entries are never shared across
// dictionaries or threads and need no invalidation.

const CACHE_WAYS: usize = 2;

#[derive(Copy, Clone, Eq, PartialEq)]
struct WordInfoCacheKey {
    word_id: u32,
    subset: u32,
}

struct WordInfoCacheEntry {
    key: WordInfoCacheKey,
    data: Arc<WordInfoData>,
    strings: Arc<StringsCache>,
}

/// Tokenizer-local cache of resolved [`WordInfo`] internals.
pub(crate) struct WordInfoCache {
    /// `buckets * CACHE_WAYS` slots, allocated lazily on the first insert so an
    /// unused or OOV-only tokenizer pays nothing.
    slots: Option<Box<[Option<WordInfoCacheEntry>]>>,
    bucket_mask: usize,
    buckets: usize,
}

impl WordInfoCache {
    /// Size the cache for roughly `capacity` entries (`CACHE_WAYS` per bucket, bucket
    /// count rounded up to a power of two). No memory is allocated until first insert.
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        let buckets = (capacity / CACHE_WAYS).max(1).next_power_of_two();
        WordInfoCache {
            slots: None,
            bucket_mask: buckets - 1,
            buckets,
        }
    }

    /// Multiplicative hash of the key to a bucket index.
    #[inline]
    fn bucket(&self, key: WordInfoCacheKey) -> usize {
        let mixed = (key.word_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (key.subset as u64).wrapping_mul(0xD1B5_4A32_D192_ED03);
        (mixed >> 32) as usize & self.bucket_mask
    }

    /// Return the cached `WordInfo` for `(word_id, subset)`, resolving and caching it
    /// on a miss. The subset is part of the key, so a narrow-subset request never
    /// receives wider cached data. Must not be called for OOV ids: their strings come
    /// from the input surface, not the dictionary.
    pub(crate) fn get_or_insert(
        &mut self,
        lexicon: &LexiconSet<'_>,
        word_id: WordId,
        subset: InfoSubset,
    ) -> SudachiResult<WordInfo> {
        assert!(
            !word_id.is_oov(),
            "OOV word ids must not be cached; build them with WordInfo::new_oov"
        );
        let key = WordInfoCacheKey {
            word_id: word_id.as_raw(),
            subset: subset.bits(),
        };
        let base = self.bucket(key) * CACHE_WAYS;
        let total = self.buckets * CACHE_WAYS;
        let ways = &mut self.slots.get_or_insert_with(|| {
            std::iter::repeat_with(|| None)
                .take(total)
                .collect::<Vec<_>>()
                .into_boxed_slice()
        })[base..base + CACHE_WAYS];

        if let Some(hit) = ways
            .iter()
            .flatten()
            .find(|entry| entry.key == key)
        {
            return Ok(WordInfo::new_shared(
                Arc::clone(&hit.data),
                Arc::clone(&hit.strings),
                word_id,
            ));
        }

        // Miss: resolve once and store, reusing an empty way or evicting the first.
        let data = Arc::new(lexicon.get_word_info_data_subset(word_id, subset)?);
        let strings = Arc::new(StringsCache::new());
        let victim = ways.iter().position(Option::is_none).unwrap_or(0);
        ways[victim] = Some(WordInfoCacheEntry {
            key,
            data: Arc::clone(&data),
            strings: Arc::clone(&strings),
        });
        Ok(WordInfo::new_shared(data, strings, word_id))
    }
}

#[cfg(test)]
mod tests {
    use super::WordInfoCache;
    use crate::dic::binary_loader::LoadedDictionary;
    use crate::dic::subset::InfoSubset;
    use crate::dic::word_id::WordId;

    const TEST_SYSTEM_DIC: &[u8] = include_bytes!("../../tests/resources/system.dic.test");

    #[test]
    fn check_prefix_ends_matches_lookup_end_order() {
        let dictionary = LoadedDictionary::load_system(TEST_SYSTEM_DIC).unwrap();
        let lexicon_set = &dictionary.lexicon_set;
        let inputs = [
            "ばな。なです。",
            "東京都に行く",
            "京都",
            "あいうえお",
            "1.と2.が。",
        ];

        for input in inputs {
            let bytes = input.as_bytes();
            for (offset, _) in input.char_indices() {
                let mut checked_ends = Vec::new();
                let decision = lexicon_set.check_prefix_ends(bytes, offset, |end| {
                    checked_ends.push(end);
                    None::<bool>
                });

                assert_eq!(decision, None);

                let mut expected_ends = Vec::new();
                for lexicon in lexicon_set.lexicons.iter().rev() {
                    let mut lookup_ends = Vec::new();
                    for entry in lexicon.lookup(bytes, offset) {
                        if lookup_ends.last() != Some(&entry.end) {
                            lookup_ends.push(entry.end);
                        }
                    }

                    let prefix_ends = lexicon
                        .lookup_prefix_ends(bytes, offset)
                        .collect::<Vec<_>>();
                    assert_eq!(lookup_ends, prefix_ends);
                    expected_ends.extend(prefix_ends);
                }

                assert_eq!(checked_ends, expected_ends);
            }
        }
    }

    /// The key is the exact `(word_id, subset)` pair: a slot filled by a narrow
    /// subset request must not satisfy a wider request for the same word id.
    #[test]
    fn cache_key_is_subset_exact() {
        let dic = LoadedDictionary::load_system(TEST_SYSTEM_DIC).unwrap();
        let lex = &dic.lexicon_set;
        // 東京都 (word id index 6) carries an A-unit split (東京 / 都).
        let wid = *lex.system_word_ids_in_order().get(6).unwrap();
        let full = InfoSubset::all();

        let truth = lex.get_word_info_subset(wid, full).unwrap();
        let truth_split = truth.a_unit_split().to_vec();
        assert!(!truth_split.is_empty(), "fixture 東京都 must have an A-split");

        let mut cache = WordInfoCache::with_capacity(64);
        // Poison the slot with a POS_ID-only entry, then request the full subset.
        let poison = cache
            .get_or_insert(lex, wid, InfoSubset::POS_ID)
            .unwrap();
        assert!(poison.a_unit_split().is_empty());
        let after = cache.get_or_insert(lex, wid, full).unwrap();
        assert_eq!(
            truth_split,
            after.a_unit_split(),
            "a POS_ID-only entry must not satisfy an all()-subset request"
        );
    }

    /// A word whose normalized form references another word's headword resolves to
    /// that headword through the cache, and a warm hit returns the identical value.
    #[test]
    fn normalized_form_cache_shares_referenced_headword() {
        let dic = LoadedDictionary::load_system(TEST_SYSTEM_DIC).unwrap();
        let lex = &dic.lexicon_set;
        let subset = InfoSubset::all();
        // 行っ normalizes to 行く (a reference to a different word's headword).
        let wid = lex
            .system_word_ids_in_order()
            .into_iter()
            .find(|&id| {
                let wi = lex.get_word_info_subset(id, subset).unwrap();
                wi.borrow_data().normalized_form_word_id() != id
            })
            .expect("fixture must contain a referenced normalized form (行っ→行く)");

        let truth = lex
            .get_word_info_subset(wid, subset)
            .unwrap()
            .normalized_form(lex)
            .to_string();

        let mut cache = WordInfoCache::with_capacity(64);
        let first = cache.get_or_insert(lex, wid, subset).unwrap();
        assert_eq!(truth, first.normalized_form(lex));
        let second = cache.get_or_insert(lex, wid, subset).unwrap();
        assert_eq!(truth, second.normalized_form(lex));
        assert_ne!(first.headword(lex), first.normalized_form(lex));
    }

    /// Every cached lookup must equal the uncached resolution even under extreme
    /// eviction (1 bucket × 2 ways), so neither hash collisions nor evictions can
    /// ever return another word's data.
    #[test]
    fn cache_stays_correct_under_heavy_eviction() {
        let dic = LoadedDictionary::load_system(TEST_SYSTEM_DIC).unwrap();
        let lex = &dic.lexicon_set;
        let subset = InfoSubset::all();
        let mut cache = WordInfoCache::with_capacity(2); // 1 bucket, 2 ways
        for &wid in lex.system_word_ids_in_order().iter() {
            let cached = cache.get_or_insert(lex, wid, subset).unwrap();
            let truth = lex.get_word_info_subset(wid, subset).unwrap();
            assert_eq!(cached.pos_id(), truth.pos_id());
            assert_eq!(cached.normalized_form(lex), truth.normalized_form(lex));
            assert_eq!(cached.a_unit_split(), truth.a_unit_split());
        }
    }

    /// With room for every entry, the warm second pass is all hits and must still
    /// return each word's own data.
    #[test]
    fn cache_serves_warm_hits_correctly() {
        let dic = LoadedDictionary::load_system(TEST_SYSTEM_DIC).unwrap();
        let lex = &dic.lexicon_set;
        let subset = InfoSubset::all();
        let wids = lex.system_word_ids_in_order();
        let mut cache = WordInfoCache::with_capacity(4096);
        for _pass in 0..2 {
            for &wid in wids.iter() {
                let cached = cache.get_or_insert(lex, wid, subset).unwrap();
                let truth = lex.get_word_info_subset(wid, subset).unwrap();
                assert_eq!(cached.normalized_form(lex), truth.normalized_form(lex));
                assert_eq!(cached.reading_form(lex), truth.reading_form(lex));
            }
        }
    }

    /// The reverse of `cache_key_is_subset_exact`: a full-subset entry must not
    /// satisfy a narrower request (no superset hit).
    #[test]
    fn cache_full_entry_does_not_satisfy_narrow_request() {
        let dic = LoadedDictionary::load_system(TEST_SYSTEM_DIC).unwrap();
        let lex = &dic.lexicon_set;
        let wid = *lex.system_word_ids_in_order().get(6).unwrap(); // 東京都 (A-split)
        let mut cache = WordInfoCache::with_capacity(64);
        let full = cache.get_or_insert(lex, wid, InfoSubset::all()).unwrap();
        assert!(!full.a_unit_split().is_empty());
        let narrow = cache.get_or_insert(lex, wid, InfoSubset::POS_ID).unwrap();
        let truth = lex.get_word_info_subset(wid, InfoSubset::POS_ID).unwrap();
        assert_eq!(narrow.a_unit_split(), truth.a_unit_split());
        assert!(narrow.a_unit_split().is_empty());
    }

    /// OOV ids must be rejected — their strings come from the input surface, so
    /// caching them by word id would serve the wrong surface's strings.
    #[test]
    #[should_panic(expected = "OOV")]
    fn oov_id_is_rejected() {
        let dic = LoadedDictionary::load_system(TEST_SYSTEM_DIC).unwrap();
        let lex = &dic.lexicon_set;
        let mut cache = WordInfoCache::with_capacity(16);
        let _ = cache.get_or_insert(lex, WordId::oov(0), InfoSubset::all());
    }

    /// `with_capacity` must round any size (incl. 0, 1, non-powers-of-two) to a
    /// usable table.
    #[test]
    fn with_capacity_handles_small_and_odd_sizes() {
        let dic = LoadedDictionary::load_system(TEST_SYSTEM_DIC).unwrap();
        let lex = &dic.lexicon_set;
        let wid = *lex.system_word_ids_in_order().first().unwrap();
        let subset = InfoSubset::all();
        for cap in [0usize, 1, 3, 7, 31] {
            let mut cache = WordInfoCache::with_capacity(cap);
            let miss = cache.get_or_insert(lex, wid, subset).unwrap();
            let hit = cache.get_or_insert(lex, wid, subset).unwrap();
            assert_eq!(miss.normalized_form(lex), hit.normalized_form(lex));
        }
    }
}
