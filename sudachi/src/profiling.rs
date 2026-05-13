/*
 *  Copyright (c) 2021-2024 Works Applications Co., Ltd.
 *
 *  Licensed under the Apache License, Version 2.0 (the "License");
 *  you may not use this file except in compliance with the License.
 *  You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 *   Unless required by applicable law or agreed to in writing, software
 *  distributed under the License is distributed on an "AS IS" BASIS,
 *  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 *  See the License for the specific language governing permissions and
 *  limitations under the License.
 */

use std::sync::atomic::{AtomicU64, Ordering};

use crate::analysis::created::CreatedWords;
use crate::analysis::Node;
use crate::dic::category_type::CategoryType;
use crate::dic::subset::InfoSubset;

#[derive(Clone, Copy, Debug, Default)]
pub struct WordInfoCounters {
    pub word_info_requests: u64,
    pub word_info_cache_hits: u64,
    pub word_info_decodes: u64,
    pub pos_decodes: u64,
    pub normalized_form_decodes: u64,
    pub dictionary_form_decodes: u64,
    pub reading_form_decodes: u64,
    pub split_a_decodes: u64,
    pub split_b_decodes: u64,
    pub split_c_decodes: u64,
    pub owned_string_allocations: u64,
    pub vec_allocations: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LatticeCounters {
    pub inserted_nodes: u64,
    pub bos_nodes: u64,
    pub eos_nodes: u64,
    pub reset_calls: u64,
    pub reset_vecs_visited: u64,
    pub reset_items_cleared: u64,
    pub reset_new_boundaries: u64,
    pub boundaries_touched: u64,
    pub boundaries_empty: u64,
    pub max_nodes_per_boundary: u64,
    pub total_nodes_per_boundary: u64,
    pub boundary_width_1: u64,
    pub boundary_width_2_4: u64,
    pub boundary_width_5_8: u64,
    pub boundary_width_9_16: u64,
    pub boundary_width_17_32: u64,
    pub boundary_width_33_64: u64,
    pub boundary_width_65_128: u64,
    pub boundary_width_129_plus: u64,
    pub left_connection_checks: u64,
    pub right_connection_checks: u64,
    pub cost_updates: u64,
    pub node_vec_growths: u64,
    pub edge_vec_growths: u64,
    pub rejected_candidates: u64,
    pub best_prev_calls: u64,
    pub best_prev_fast_empty: u64,
    pub best_prev_fast_single: u64,
    pub best_prev_direct_small: u64,
    pub prev_nodes_0: u64,
    pub prev_nodes_1: u64,
    pub prev_nodes_2_4: u64,
    pub prev_nodes_5_16: u64,
    pub prev_nodes_17_plus: u64,
    pub left_id_cache_probes: u64,
    pub left_id_cache_hits: u64,
    pub left_id_cache_misses: u64,
    pub left_id_cache_saved_checks: u64,
    pub left_id_cache_miss_checks: u64,
    pub left_id_boundaries: u64,
    pub left_id_total_unique: u64,
    pub left_id_max_unique_per_boundary: u64,
    pub best_prev_cache_pushes: u64,
    pub best_prev_cache_capacity_total: u64,
    pub ends_capacity_total: u64,
    pub ends_full_capacity_total: u64,
    pub indices_capacity_total: u64,
    pub positions_total: u64,
    pub positions_reachable: u64,
    pub positions_unreachable_skipped: u64,
    pub direct_inserted_candidates: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OovCounters {
    pub provider_calls: u64,
    pub candidates_provided: u64,
    pub inserted_nodes: u64,
    pub duplicate_candidates: u64,
    pub best_path_nodes: u64,
    pub mecab_provider_calls: u64,
    pub simple_provider_calls: u64,
    pub regex_provider_calls: u64,
    pub other_provider_calls: u64,
    pub mecab_candidates: u64,
    pub simple_candidates: u64,
    pub regex_candidates: u64,
    pub other_candidates: u64,
    pub single_candidates: u64,
    pub grouped_candidates: u64,
    pub length_1: u64,
    pub length_2: u64,
    pub length_3: u64,
    pub length_4: u64,
    pub length_5_8: u64,
    pub length_9_16: u64,
    pub length_17_32: u64,
    pub length_33_plus: u64,
    pub category_default: u64,
    pub category_space: u64,
    pub category_kanji: u64,
    pub category_symbol: u64,
    pub category_numeric: u64,
    pub category_alpha: u64,
    pub category_hiragana: u64,
    pub category_katakana: u64,
    pub category_kanjinumeric: u64,
    pub category_greek: u64,
    pub category_cyrillic: u64,
    pub category_user1: u64,
    pub category_user2: u64,
    pub category_user3: u64,
    pub category_user4: u64,
    pub category_other: u64,
    pub ranges_total: u64,
    pub range_candidates_total: u64,
    pub range_max_candidates: u64,
    pub range_0: u64,
    pub range_1: u64,
    pub range_2_4: u64,
    pub range_5_8: u64,
    pub range_9_16: u64,
    pub range_17_23: u64,
    pub range_24_plus: u64,
    pub suppressed_by_has_other_words: u64,
    pub suppressed_by_invoke_false: u64,
    pub suppressed_group_by_group_false: u64,
    pub buffered_provider_calls: u64,
    pub buffered_candidates: u64,
    pub temp_buffer_growths: u64,
    pub temp_buffer_max_len: u64,
    pub dominance_groups: u64,
    pub strict_dominated_candidates: u64,
    pub equal_cost_ties: u64,
    pub unique_after_dominance: u64,
    pub dominated_length_1: u64,
    pub dominated_length_2: u64,
    pub dominated_length_3: u64,
    pub dominated_length_4: u64,
    pub dominated_length_5_8: u64,
    pub dominated_length_9_16: u64,
    pub dominated_length_17_32: u64,
    pub dominated_length_33_plus: u64,
    pub dominated_category_default: u64,
    pub dominated_category_space: u64,
    pub dominated_category_kanji: u64,
    pub dominated_category_symbol: u64,
    pub dominated_category_numeric: u64,
    pub dominated_category_alpha: u64,
    pub dominated_category_hiragana: u64,
    pub dominated_category_katakana: u64,
    pub dominated_category_kanjinumeric: u64,
    pub dominated_category_greek: u64,
    pub dominated_category_cyrillic: u64,
    pub dominated_category_user1: u64,
    pub dominated_category_user2: u64,
    pub dominated_category_user3: u64,
    pub dominated_category_user4: u64,
    pub dominated_category_other: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TypeSizeCounters {
    pub node: u64,
    pub vnode: u64,
    pub best_prev: u64,
    pub created_words: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Counters {
    pub word_info: WordInfoCounters,
    pub lattice: LatticeCounters,
    pub oov: OovCounters,
    pub type_sizes: TypeSizeCounters,
}

macro_rules! counter {
    ($name:ident) => {
        static $name: AtomicU64 = AtomicU64::new(0);
    };
}

counter!(WORD_INFO_REQUESTS);
counter!(WORD_INFO_CACHE_HITS);
counter!(WORD_INFO_DECODES);
counter!(POS_DECODES);
counter!(NORMALIZED_FORM_DECODES);
counter!(DICTIONARY_FORM_DECODES);
counter!(READING_FORM_DECODES);
counter!(SPLIT_A_DECODES);
counter!(SPLIT_B_DECODES);
counter!(SPLIT_C_DECODES);
counter!(OWNED_STRING_ALLOCATIONS);
counter!(VEC_ALLOCATIONS);

counter!(INSERTED_NODES);
counter!(BOS_NODES);
counter!(EOS_NODES);
counter!(RESET_CALLS);
counter!(RESET_VECS_VISITED);
counter!(RESET_ITEMS_CLEARED);
counter!(RESET_NEW_BOUNDARIES);
counter!(BOUNDARIES_TOUCHED);
counter!(BOUNDARIES_EMPTY);
counter!(MAX_NODES_PER_BOUNDARY);
counter!(TOTAL_NODES_PER_BOUNDARY);
counter!(BOUNDARY_WIDTH_1);
counter!(BOUNDARY_WIDTH_2_4);
counter!(BOUNDARY_WIDTH_5_8);
counter!(BOUNDARY_WIDTH_9_16);
counter!(BOUNDARY_WIDTH_17_32);
counter!(BOUNDARY_WIDTH_33_64);
counter!(BOUNDARY_WIDTH_65_128);
counter!(BOUNDARY_WIDTH_129_PLUS);
counter!(LEFT_CONNECTION_CHECKS);
counter!(RIGHT_CONNECTION_CHECKS);
counter!(COST_UPDATES);
counter!(NODE_VEC_GROWTHS);
counter!(EDGE_VEC_GROWTHS);
counter!(REJECTED_CANDIDATES);
counter!(BEST_PREV_CALLS);
counter!(BEST_PREV_FAST_EMPTY);
counter!(BEST_PREV_FAST_SINGLE);
counter!(BEST_PREV_DIRECT_SMALL);
counter!(PREV_NODES_0);
counter!(PREV_NODES_1);
counter!(PREV_NODES_2_4);
counter!(PREV_NODES_5_16);
counter!(PREV_NODES_17_PLUS);
counter!(LEFT_ID_CACHE_PROBES);
counter!(LEFT_ID_CACHE_HITS);
counter!(LEFT_ID_CACHE_MISSES);
counter!(LEFT_ID_CACHE_SAVED_CHECKS);
counter!(LEFT_ID_CACHE_MISS_CHECKS);
counter!(LEFT_ID_BOUNDARIES);
counter!(LEFT_ID_TOTAL_UNIQUE);
counter!(LEFT_ID_MAX_UNIQUE_PER_BOUNDARY);
counter!(BEST_PREV_CACHE_PUSHES);
counter!(BEST_PREV_CACHE_CAPACITY_TOTAL);
counter!(ENDS_CAPACITY_TOTAL);
counter!(ENDS_FULL_CAPACITY_TOTAL);
counter!(INDICES_CAPACITY_TOTAL);
counter!(POSITIONS_TOTAL);
counter!(POSITIONS_REACHABLE);
counter!(POSITIONS_UNREACHABLE_SKIPPED);
counter!(DIRECT_INSERTED_CANDIDATES);

counter!(OOV_PROVIDER_CALLS);
counter!(OOV_CANDIDATES_PROVIDED);
counter!(OOV_INSERTED_NODES);
counter!(OOV_DUPLICATE_CANDIDATES);
counter!(OOV_BEST_PATH_NODES);
counter!(OOV_MECAB_PROVIDER_CALLS);
counter!(OOV_SIMPLE_PROVIDER_CALLS);
counter!(OOV_REGEX_PROVIDER_CALLS);
counter!(OOV_OTHER_PROVIDER_CALLS);
counter!(OOV_MECAB_CANDIDATES);
counter!(OOV_SIMPLE_CANDIDATES);
counter!(OOV_REGEX_CANDIDATES);
counter!(OOV_OTHER_CANDIDATES);
counter!(OOV_SINGLE_CANDIDATES);
counter!(OOV_GROUPED_CANDIDATES);
counter!(OOV_LENGTH_1);
counter!(OOV_LENGTH_2);
counter!(OOV_LENGTH_3);
counter!(OOV_LENGTH_4);
counter!(OOV_LENGTH_5_8);
counter!(OOV_LENGTH_9_16);
counter!(OOV_LENGTH_17_32);
counter!(OOV_LENGTH_33_PLUS);
counter!(OOV_CATEGORY_DEFAULT);
counter!(OOV_CATEGORY_SPACE);
counter!(OOV_CATEGORY_KANJI);
counter!(OOV_CATEGORY_SYMBOL);
counter!(OOV_CATEGORY_NUMERIC);
counter!(OOV_CATEGORY_ALPHA);
counter!(OOV_CATEGORY_HIRAGANA);
counter!(OOV_CATEGORY_KATAKANA);
counter!(OOV_CATEGORY_KANJINUMERIC);
counter!(OOV_CATEGORY_GREEK);
counter!(OOV_CATEGORY_CYRILLIC);
counter!(OOV_CATEGORY_USER1);
counter!(OOV_CATEGORY_USER2);
counter!(OOV_CATEGORY_USER3);
counter!(OOV_CATEGORY_USER4);
counter!(OOV_CATEGORY_OTHER);
counter!(OOV_RANGES_TOTAL);
counter!(OOV_RANGE_CANDIDATES_TOTAL);
counter!(OOV_RANGE_MAX_CANDIDATES);
counter!(OOV_RANGE_0);
counter!(OOV_RANGE_1);
counter!(OOV_RANGE_2_4);
counter!(OOV_RANGE_5_8);
counter!(OOV_RANGE_9_16);
counter!(OOV_RANGE_17_23);
counter!(OOV_RANGE_24_PLUS);
counter!(OOV_SUPPRESSED_BY_HAS_OTHER_WORDS);
counter!(OOV_SUPPRESSED_BY_INVOKE_FALSE);
counter!(OOV_SUPPRESSED_GROUP_BY_GROUP_FALSE);
counter!(OOV_BUFFERED_PROVIDER_CALLS);
counter!(OOV_BUFFERED_CANDIDATES);
counter!(OOV_TEMP_BUFFER_GROWTHS);
counter!(OOV_TEMP_BUFFER_MAX_LEN);
counter!(OOV_DOMINANCE_GROUPS);
counter!(OOV_STRICT_DOMINATED_CANDIDATES);
counter!(OOV_EQUAL_COST_TIES);
counter!(OOV_UNIQUE_AFTER_DOMINANCE);
counter!(OOV_DOMINATED_LENGTH_1);
counter!(OOV_DOMINATED_LENGTH_2);
counter!(OOV_DOMINATED_LENGTH_3);
counter!(OOV_DOMINATED_LENGTH_4);
counter!(OOV_DOMINATED_LENGTH_5_8);
counter!(OOV_DOMINATED_LENGTH_9_16);
counter!(OOV_DOMINATED_LENGTH_17_32);
counter!(OOV_DOMINATED_LENGTH_33_PLUS);
counter!(OOV_DOMINATED_CATEGORY_DEFAULT);
counter!(OOV_DOMINATED_CATEGORY_SPACE);
counter!(OOV_DOMINATED_CATEGORY_KANJI);
counter!(OOV_DOMINATED_CATEGORY_SYMBOL);
counter!(OOV_DOMINATED_CATEGORY_NUMERIC);
counter!(OOV_DOMINATED_CATEGORY_ALPHA);
counter!(OOV_DOMINATED_CATEGORY_HIRAGANA);
counter!(OOV_DOMINATED_CATEGORY_KATAKANA);
counter!(OOV_DOMINATED_CATEGORY_KANJINUMERIC);
counter!(OOV_DOMINATED_CATEGORY_GREEK);
counter!(OOV_DOMINATED_CATEGORY_CYRILLIC);
counter!(OOV_DOMINATED_CATEGORY_USER1);
counter!(OOV_DOMINATED_CATEGORY_USER2);
counter!(OOV_DOMINATED_CATEGORY_USER3);
counter!(OOV_DOMINATED_CATEGORY_USER4);
counter!(OOV_DOMINATED_CATEGORY_OTHER);

macro_rules! reset_counter {
    ($name:ident) => {
        $name.store(0, Ordering::Relaxed);
    };
}

macro_rules! load_counter {
    ($name:ident) => {
        $name.load(Ordering::Relaxed)
    };
}

#[inline]
fn add(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

#[inline]
fn max(counter: &AtomicU64, value: u64) {
    let mut current = counter.load(Ordering::Relaxed);
    while value > current {
        match counter.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

pub fn reset() {
    reset_counter!(WORD_INFO_REQUESTS);
    reset_counter!(WORD_INFO_CACHE_HITS);
    reset_counter!(WORD_INFO_DECODES);
    reset_counter!(POS_DECODES);
    reset_counter!(NORMALIZED_FORM_DECODES);
    reset_counter!(DICTIONARY_FORM_DECODES);
    reset_counter!(READING_FORM_DECODES);
    reset_counter!(SPLIT_A_DECODES);
    reset_counter!(SPLIT_B_DECODES);
    reset_counter!(SPLIT_C_DECODES);
    reset_counter!(OWNED_STRING_ALLOCATIONS);
    reset_counter!(VEC_ALLOCATIONS);

    reset_counter!(INSERTED_NODES);
    reset_counter!(BOS_NODES);
    reset_counter!(EOS_NODES);
    reset_counter!(RESET_CALLS);
    reset_counter!(RESET_VECS_VISITED);
    reset_counter!(RESET_ITEMS_CLEARED);
    reset_counter!(RESET_NEW_BOUNDARIES);
    reset_counter!(BOUNDARIES_TOUCHED);
    reset_counter!(BOUNDARIES_EMPTY);
    reset_counter!(MAX_NODES_PER_BOUNDARY);
    reset_counter!(TOTAL_NODES_PER_BOUNDARY);
    reset_counter!(BOUNDARY_WIDTH_1);
    reset_counter!(BOUNDARY_WIDTH_2_4);
    reset_counter!(BOUNDARY_WIDTH_5_8);
    reset_counter!(BOUNDARY_WIDTH_9_16);
    reset_counter!(BOUNDARY_WIDTH_17_32);
    reset_counter!(BOUNDARY_WIDTH_33_64);
    reset_counter!(BOUNDARY_WIDTH_65_128);
    reset_counter!(BOUNDARY_WIDTH_129_PLUS);
    reset_counter!(LEFT_CONNECTION_CHECKS);
    reset_counter!(RIGHT_CONNECTION_CHECKS);
    reset_counter!(COST_UPDATES);
    reset_counter!(NODE_VEC_GROWTHS);
    reset_counter!(EDGE_VEC_GROWTHS);
    reset_counter!(REJECTED_CANDIDATES);
    reset_counter!(BEST_PREV_CALLS);
    reset_counter!(BEST_PREV_FAST_EMPTY);
    reset_counter!(BEST_PREV_FAST_SINGLE);
    reset_counter!(BEST_PREV_DIRECT_SMALL);
    reset_counter!(PREV_NODES_0);
    reset_counter!(PREV_NODES_1);
    reset_counter!(PREV_NODES_2_4);
    reset_counter!(PREV_NODES_5_16);
    reset_counter!(PREV_NODES_17_PLUS);
    reset_counter!(LEFT_ID_CACHE_PROBES);
    reset_counter!(LEFT_ID_CACHE_HITS);
    reset_counter!(LEFT_ID_CACHE_MISSES);
    reset_counter!(LEFT_ID_CACHE_SAVED_CHECKS);
    reset_counter!(LEFT_ID_CACHE_MISS_CHECKS);
    reset_counter!(LEFT_ID_BOUNDARIES);
    reset_counter!(LEFT_ID_TOTAL_UNIQUE);
    reset_counter!(LEFT_ID_MAX_UNIQUE_PER_BOUNDARY);
    reset_counter!(BEST_PREV_CACHE_PUSHES);
    reset_counter!(BEST_PREV_CACHE_CAPACITY_TOTAL);
    reset_counter!(ENDS_CAPACITY_TOTAL);
    reset_counter!(ENDS_FULL_CAPACITY_TOTAL);
    reset_counter!(INDICES_CAPACITY_TOTAL);
    reset_counter!(POSITIONS_TOTAL);
    reset_counter!(POSITIONS_REACHABLE);
    reset_counter!(POSITIONS_UNREACHABLE_SKIPPED);
    reset_counter!(DIRECT_INSERTED_CANDIDATES);

    reset_counter!(OOV_PROVIDER_CALLS);
    reset_counter!(OOV_CANDIDATES_PROVIDED);
    reset_counter!(OOV_INSERTED_NODES);
    reset_counter!(OOV_DUPLICATE_CANDIDATES);
    reset_counter!(OOV_BEST_PATH_NODES);
    reset_counter!(OOV_MECAB_PROVIDER_CALLS);
    reset_counter!(OOV_SIMPLE_PROVIDER_CALLS);
    reset_counter!(OOV_REGEX_PROVIDER_CALLS);
    reset_counter!(OOV_OTHER_PROVIDER_CALLS);
    reset_counter!(OOV_MECAB_CANDIDATES);
    reset_counter!(OOV_SIMPLE_CANDIDATES);
    reset_counter!(OOV_REGEX_CANDIDATES);
    reset_counter!(OOV_OTHER_CANDIDATES);
    reset_counter!(OOV_SINGLE_CANDIDATES);
    reset_counter!(OOV_GROUPED_CANDIDATES);
    reset_counter!(OOV_LENGTH_1);
    reset_counter!(OOV_LENGTH_2);
    reset_counter!(OOV_LENGTH_3);
    reset_counter!(OOV_LENGTH_4);
    reset_counter!(OOV_LENGTH_5_8);
    reset_counter!(OOV_LENGTH_9_16);
    reset_counter!(OOV_LENGTH_17_32);
    reset_counter!(OOV_LENGTH_33_PLUS);
    reset_counter!(OOV_CATEGORY_DEFAULT);
    reset_counter!(OOV_CATEGORY_SPACE);
    reset_counter!(OOV_CATEGORY_KANJI);
    reset_counter!(OOV_CATEGORY_SYMBOL);
    reset_counter!(OOV_CATEGORY_NUMERIC);
    reset_counter!(OOV_CATEGORY_ALPHA);
    reset_counter!(OOV_CATEGORY_HIRAGANA);
    reset_counter!(OOV_CATEGORY_KATAKANA);
    reset_counter!(OOV_CATEGORY_KANJINUMERIC);
    reset_counter!(OOV_CATEGORY_GREEK);
    reset_counter!(OOV_CATEGORY_CYRILLIC);
    reset_counter!(OOV_CATEGORY_USER1);
    reset_counter!(OOV_CATEGORY_USER2);
    reset_counter!(OOV_CATEGORY_USER3);
    reset_counter!(OOV_CATEGORY_USER4);
    reset_counter!(OOV_CATEGORY_OTHER);
    reset_counter!(OOV_RANGES_TOTAL);
    reset_counter!(OOV_RANGE_CANDIDATES_TOTAL);
    reset_counter!(OOV_RANGE_MAX_CANDIDATES);
    reset_counter!(OOV_RANGE_0);
    reset_counter!(OOV_RANGE_1);
    reset_counter!(OOV_RANGE_2_4);
    reset_counter!(OOV_RANGE_5_8);
    reset_counter!(OOV_RANGE_9_16);
    reset_counter!(OOV_RANGE_17_23);
    reset_counter!(OOV_RANGE_24_PLUS);
    reset_counter!(OOV_SUPPRESSED_BY_HAS_OTHER_WORDS);
    reset_counter!(OOV_SUPPRESSED_BY_INVOKE_FALSE);
    reset_counter!(OOV_SUPPRESSED_GROUP_BY_GROUP_FALSE);
    reset_counter!(OOV_BUFFERED_PROVIDER_CALLS);
    reset_counter!(OOV_BUFFERED_CANDIDATES);
    reset_counter!(OOV_TEMP_BUFFER_GROWTHS);
    reset_counter!(OOV_TEMP_BUFFER_MAX_LEN);
    reset_counter!(OOV_DOMINANCE_GROUPS);
    reset_counter!(OOV_STRICT_DOMINATED_CANDIDATES);
    reset_counter!(OOV_EQUAL_COST_TIES);
    reset_counter!(OOV_UNIQUE_AFTER_DOMINANCE);
    reset_counter!(OOV_DOMINATED_LENGTH_1);
    reset_counter!(OOV_DOMINATED_LENGTH_2);
    reset_counter!(OOV_DOMINATED_LENGTH_3);
    reset_counter!(OOV_DOMINATED_LENGTH_4);
    reset_counter!(OOV_DOMINATED_LENGTH_5_8);
    reset_counter!(OOV_DOMINATED_LENGTH_9_16);
    reset_counter!(OOV_DOMINATED_LENGTH_17_32);
    reset_counter!(OOV_DOMINATED_LENGTH_33_PLUS);
    reset_counter!(OOV_DOMINATED_CATEGORY_DEFAULT);
    reset_counter!(OOV_DOMINATED_CATEGORY_SPACE);
    reset_counter!(OOV_DOMINATED_CATEGORY_KANJI);
    reset_counter!(OOV_DOMINATED_CATEGORY_SYMBOL);
    reset_counter!(OOV_DOMINATED_CATEGORY_NUMERIC);
    reset_counter!(OOV_DOMINATED_CATEGORY_ALPHA);
    reset_counter!(OOV_DOMINATED_CATEGORY_HIRAGANA);
    reset_counter!(OOV_DOMINATED_CATEGORY_KATAKANA);
    reset_counter!(OOV_DOMINATED_CATEGORY_KANJINUMERIC);
    reset_counter!(OOV_DOMINATED_CATEGORY_GREEK);
    reset_counter!(OOV_DOMINATED_CATEGORY_CYRILLIC);
    reset_counter!(OOV_DOMINATED_CATEGORY_USER1);
    reset_counter!(OOV_DOMINATED_CATEGORY_USER2);
    reset_counter!(OOV_DOMINATED_CATEGORY_USER3);
    reset_counter!(OOV_DOMINATED_CATEGORY_USER4);
    reset_counter!(OOV_DOMINATED_CATEGORY_OTHER);
}

pub fn snapshot() -> Counters {
    Counters {
        word_info: WordInfoCounters {
            word_info_requests: load_counter!(WORD_INFO_REQUESTS),
            word_info_cache_hits: load_counter!(WORD_INFO_CACHE_HITS),
            word_info_decodes: load_counter!(WORD_INFO_DECODES),
            pos_decodes: load_counter!(POS_DECODES),
            normalized_form_decodes: load_counter!(NORMALIZED_FORM_DECODES),
            dictionary_form_decodes: load_counter!(DICTIONARY_FORM_DECODES),
            reading_form_decodes: load_counter!(READING_FORM_DECODES),
            split_a_decodes: load_counter!(SPLIT_A_DECODES),
            split_b_decodes: load_counter!(SPLIT_B_DECODES),
            split_c_decodes: load_counter!(SPLIT_C_DECODES),
            owned_string_allocations: load_counter!(OWNED_STRING_ALLOCATIONS),
            vec_allocations: load_counter!(VEC_ALLOCATIONS),
        },
        lattice: LatticeCounters {
            inserted_nodes: load_counter!(INSERTED_NODES),
            bos_nodes: load_counter!(BOS_NODES),
            eos_nodes: load_counter!(EOS_NODES),
            reset_calls: load_counter!(RESET_CALLS),
            reset_vecs_visited: load_counter!(RESET_VECS_VISITED),
            reset_items_cleared: load_counter!(RESET_ITEMS_CLEARED),
            reset_new_boundaries: load_counter!(RESET_NEW_BOUNDARIES),
            boundaries_touched: load_counter!(BOUNDARIES_TOUCHED),
            boundaries_empty: load_counter!(BOUNDARIES_EMPTY),
            max_nodes_per_boundary: load_counter!(MAX_NODES_PER_BOUNDARY),
            total_nodes_per_boundary: load_counter!(TOTAL_NODES_PER_BOUNDARY),
            boundary_width_1: load_counter!(BOUNDARY_WIDTH_1),
            boundary_width_2_4: load_counter!(BOUNDARY_WIDTH_2_4),
            boundary_width_5_8: load_counter!(BOUNDARY_WIDTH_5_8),
            boundary_width_9_16: load_counter!(BOUNDARY_WIDTH_9_16),
            boundary_width_17_32: load_counter!(BOUNDARY_WIDTH_17_32),
            boundary_width_33_64: load_counter!(BOUNDARY_WIDTH_33_64),
            boundary_width_65_128: load_counter!(BOUNDARY_WIDTH_65_128),
            boundary_width_129_plus: load_counter!(BOUNDARY_WIDTH_129_PLUS),
            left_connection_checks: load_counter!(LEFT_CONNECTION_CHECKS),
            right_connection_checks: load_counter!(RIGHT_CONNECTION_CHECKS),
            cost_updates: load_counter!(COST_UPDATES),
            node_vec_growths: load_counter!(NODE_VEC_GROWTHS),
            edge_vec_growths: load_counter!(EDGE_VEC_GROWTHS),
            rejected_candidates: load_counter!(REJECTED_CANDIDATES),
            best_prev_calls: load_counter!(BEST_PREV_CALLS),
            best_prev_fast_empty: load_counter!(BEST_PREV_FAST_EMPTY),
            best_prev_fast_single: load_counter!(BEST_PREV_FAST_SINGLE),
            best_prev_direct_small: load_counter!(BEST_PREV_DIRECT_SMALL),
            prev_nodes_0: load_counter!(PREV_NODES_0),
            prev_nodes_1: load_counter!(PREV_NODES_1),
            prev_nodes_2_4: load_counter!(PREV_NODES_2_4),
            prev_nodes_5_16: load_counter!(PREV_NODES_5_16),
            prev_nodes_17_plus: load_counter!(PREV_NODES_17_PLUS),
            left_id_cache_probes: load_counter!(LEFT_ID_CACHE_PROBES),
            left_id_cache_hits: load_counter!(LEFT_ID_CACHE_HITS),
            left_id_cache_misses: load_counter!(LEFT_ID_CACHE_MISSES),
            left_id_cache_saved_checks: load_counter!(LEFT_ID_CACHE_SAVED_CHECKS),
            left_id_cache_miss_checks: load_counter!(LEFT_ID_CACHE_MISS_CHECKS),
            left_id_boundaries: load_counter!(LEFT_ID_BOUNDARIES),
            left_id_total_unique: load_counter!(LEFT_ID_TOTAL_UNIQUE),
            left_id_max_unique_per_boundary: load_counter!(LEFT_ID_MAX_UNIQUE_PER_BOUNDARY),
            best_prev_cache_pushes: load_counter!(BEST_PREV_CACHE_PUSHES),
            best_prev_cache_capacity_total: load_counter!(BEST_PREV_CACHE_CAPACITY_TOTAL),
            ends_capacity_total: load_counter!(ENDS_CAPACITY_TOTAL),
            ends_full_capacity_total: load_counter!(ENDS_FULL_CAPACITY_TOTAL),
            indices_capacity_total: load_counter!(INDICES_CAPACITY_TOTAL),
            positions_total: load_counter!(POSITIONS_TOTAL),
            positions_reachable: load_counter!(POSITIONS_REACHABLE),
            positions_unreachable_skipped: load_counter!(POSITIONS_UNREACHABLE_SKIPPED),
            direct_inserted_candidates: load_counter!(DIRECT_INSERTED_CANDIDATES),
        },
        oov: OovCounters {
            provider_calls: load_counter!(OOV_PROVIDER_CALLS),
            candidates_provided: load_counter!(OOV_CANDIDATES_PROVIDED),
            inserted_nodes: load_counter!(OOV_INSERTED_NODES),
            duplicate_candidates: load_counter!(OOV_DUPLICATE_CANDIDATES),
            best_path_nodes: load_counter!(OOV_BEST_PATH_NODES),
            mecab_provider_calls: load_counter!(OOV_MECAB_PROVIDER_CALLS),
            simple_provider_calls: load_counter!(OOV_SIMPLE_PROVIDER_CALLS),
            regex_provider_calls: load_counter!(OOV_REGEX_PROVIDER_CALLS),
            other_provider_calls: load_counter!(OOV_OTHER_PROVIDER_CALLS),
            mecab_candidates: load_counter!(OOV_MECAB_CANDIDATES),
            simple_candidates: load_counter!(OOV_SIMPLE_CANDIDATES),
            regex_candidates: load_counter!(OOV_REGEX_CANDIDATES),
            other_candidates: load_counter!(OOV_OTHER_CANDIDATES),
            single_candidates: load_counter!(OOV_SINGLE_CANDIDATES),
            grouped_candidates: load_counter!(OOV_GROUPED_CANDIDATES),
            length_1: load_counter!(OOV_LENGTH_1),
            length_2: load_counter!(OOV_LENGTH_2),
            length_3: load_counter!(OOV_LENGTH_3),
            length_4: load_counter!(OOV_LENGTH_4),
            length_5_8: load_counter!(OOV_LENGTH_5_8),
            length_9_16: load_counter!(OOV_LENGTH_9_16),
            length_17_32: load_counter!(OOV_LENGTH_17_32),
            length_33_plus: load_counter!(OOV_LENGTH_33_PLUS),
            category_default: load_counter!(OOV_CATEGORY_DEFAULT),
            category_space: load_counter!(OOV_CATEGORY_SPACE),
            category_kanji: load_counter!(OOV_CATEGORY_KANJI),
            category_symbol: load_counter!(OOV_CATEGORY_SYMBOL),
            category_numeric: load_counter!(OOV_CATEGORY_NUMERIC),
            category_alpha: load_counter!(OOV_CATEGORY_ALPHA),
            category_hiragana: load_counter!(OOV_CATEGORY_HIRAGANA),
            category_katakana: load_counter!(OOV_CATEGORY_KATAKANA),
            category_kanjinumeric: load_counter!(OOV_CATEGORY_KANJINUMERIC),
            category_greek: load_counter!(OOV_CATEGORY_GREEK),
            category_cyrillic: load_counter!(OOV_CATEGORY_CYRILLIC),
            category_user1: load_counter!(OOV_CATEGORY_USER1),
            category_user2: load_counter!(OOV_CATEGORY_USER2),
            category_user3: load_counter!(OOV_CATEGORY_USER3),
            category_user4: load_counter!(OOV_CATEGORY_USER4),
            category_other: load_counter!(OOV_CATEGORY_OTHER),
            ranges_total: load_counter!(OOV_RANGES_TOTAL),
            range_candidates_total: load_counter!(OOV_RANGE_CANDIDATES_TOTAL),
            range_max_candidates: load_counter!(OOV_RANGE_MAX_CANDIDATES),
            range_0: load_counter!(OOV_RANGE_0),
            range_1: load_counter!(OOV_RANGE_1),
            range_2_4: load_counter!(OOV_RANGE_2_4),
            range_5_8: load_counter!(OOV_RANGE_5_8),
            range_9_16: load_counter!(OOV_RANGE_9_16),
            range_17_23: load_counter!(OOV_RANGE_17_23),
            range_24_plus: load_counter!(OOV_RANGE_24_PLUS),
            suppressed_by_has_other_words: load_counter!(OOV_SUPPRESSED_BY_HAS_OTHER_WORDS),
            suppressed_by_invoke_false: load_counter!(OOV_SUPPRESSED_BY_INVOKE_FALSE),
            suppressed_group_by_group_false: load_counter!(OOV_SUPPRESSED_GROUP_BY_GROUP_FALSE),
            buffered_provider_calls: load_counter!(OOV_BUFFERED_PROVIDER_CALLS),
            buffered_candidates: load_counter!(OOV_BUFFERED_CANDIDATES),
            temp_buffer_growths: load_counter!(OOV_TEMP_BUFFER_GROWTHS),
            temp_buffer_max_len: load_counter!(OOV_TEMP_BUFFER_MAX_LEN),
            dominance_groups: load_counter!(OOV_DOMINANCE_GROUPS),
            strict_dominated_candidates: load_counter!(OOV_STRICT_DOMINATED_CANDIDATES),
            equal_cost_ties: load_counter!(OOV_EQUAL_COST_TIES),
            unique_after_dominance: load_counter!(OOV_UNIQUE_AFTER_DOMINANCE),
            dominated_length_1: load_counter!(OOV_DOMINATED_LENGTH_1),
            dominated_length_2: load_counter!(OOV_DOMINATED_LENGTH_2),
            dominated_length_3: load_counter!(OOV_DOMINATED_LENGTH_3),
            dominated_length_4: load_counter!(OOV_DOMINATED_LENGTH_4),
            dominated_length_5_8: load_counter!(OOV_DOMINATED_LENGTH_5_8),
            dominated_length_9_16: load_counter!(OOV_DOMINATED_LENGTH_9_16),
            dominated_length_17_32: load_counter!(OOV_DOMINATED_LENGTH_17_32),
            dominated_length_33_plus: load_counter!(OOV_DOMINATED_LENGTH_33_PLUS),
            dominated_category_default: load_counter!(OOV_DOMINATED_CATEGORY_DEFAULT),
            dominated_category_space: load_counter!(OOV_DOMINATED_CATEGORY_SPACE),
            dominated_category_kanji: load_counter!(OOV_DOMINATED_CATEGORY_KANJI),
            dominated_category_symbol: load_counter!(OOV_DOMINATED_CATEGORY_SYMBOL),
            dominated_category_numeric: load_counter!(OOV_DOMINATED_CATEGORY_NUMERIC),
            dominated_category_alpha: load_counter!(OOV_DOMINATED_CATEGORY_ALPHA),
            dominated_category_hiragana: load_counter!(OOV_DOMINATED_CATEGORY_HIRAGANA),
            dominated_category_katakana: load_counter!(OOV_DOMINATED_CATEGORY_KATAKANA),
            dominated_category_kanjinumeric: load_counter!(OOV_DOMINATED_CATEGORY_KANJINUMERIC),
            dominated_category_greek: load_counter!(OOV_DOMINATED_CATEGORY_GREEK),
            dominated_category_cyrillic: load_counter!(OOV_DOMINATED_CATEGORY_CYRILLIC),
            dominated_category_user1: load_counter!(OOV_DOMINATED_CATEGORY_USER1),
            dominated_category_user2: load_counter!(OOV_DOMINATED_CATEGORY_USER2),
            dominated_category_user3: load_counter!(OOV_DOMINATED_CATEGORY_USER3),
            dominated_category_user4: load_counter!(OOV_DOMINATED_CATEGORY_USER4),
            dominated_category_other: load_counter!(OOV_DOMINATED_CATEGORY_OTHER),
        },
        type_sizes: TypeSizeCounters {
            node: std::mem::size_of::<Node>() as u64,
            vnode: crate::analysis::lattice::vnode_size() as u64,
            best_prev: crate::analysis::lattice::best_prev_size() as u64,
            created_words: std::mem::size_of::<CreatedWords>() as u64,
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OovProviderKind {
    MeCab,
    Simple,
    Regex,
    Other,
}

#[inline]
pub(crate) fn count_word_info_request() {
    add(&WORD_INFO_REQUESTS, 1);
}

#[inline]
pub(crate) fn count_word_info_decode(subset: InfoSubset) {
    add(&WORD_INFO_DECODES, 1);
    if subset.contains(InfoSubset::POS_ID) {
        add(&POS_DECODES, 1);
    }
    if subset.contains(InfoSubset::NORMALIZED_FORM) {
        add(&NORMALIZED_FORM_DECODES, 1);
    }
    if subset.contains(InfoSubset::DIC_FORM_WORD_ID) {
        add(&DICTIONARY_FORM_DECODES, 1);
    }
    if subset.contains(InfoSubset::READING_FORM) {
        add(&READING_FORM_DECODES, 1);
    }
    if subset.contains(InfoSubset::SPLIT_A) {
        add(&SPLIT_A_DECODES, 1);
    }
    if subset.contains(InfoSubset::SPLIT_B) {
        add(&SPLIT_B_DECODES, 1);
    }
    if subset.contains(InfoSubset::WORD_STRUCTURE) {
        add(&SPLIT_C_DECODES, 1);
    }
}

#[inline]
pub(crate) fn count_owned_string_allocation() {
    add(&OWNED_STRING_ALLOCATIONS, 1);
}

#[inline]
pub(crate) fn count_vec_allocation() {
    add(&VEC_ALLOCATIONS, 1);
}

#[inline]
pub(crate) fn count_inserted_node() {
    add(&INSERTED_NODES, 1);
}

#[inline]
pub(crate) fn count_bos_node() {
    add(&BOS_NODES, 1);
}

#[inline]
pub(crate) fn count_eos_node() {
    add(&EOS_NODES, 1);
}

#[inline]
pub(crate) fn count_lattice_reset(
    vecs_visited: usize,
    items_cleared: usize,
    new_boundaries: usize,
) {
    add(&RESET_CALLS, 1);
    add(&RESET_VECS_VISITED, vecs_visited as u64);
    add(&RESET_ITEMS_CLEARED, items_cleared as u64);
    add(&RESET_NEW_BOUNDARIES, new_boundaries as u64);
}

#[inline]
pub(crate) fn count_boundary_nodes(nodes: usize) {
    if nodes == 0 {
        add(&BOUNDARIES_EMPTY, 1);
        return;
    }
    add(&BOUNDARIES_TOUCHED, 1);
    add(&TOTAL_NODES_PER_BOUNDARY, nodes as u64);
    max(&MAX_NODES_PER_BOUNDARY, nodes as u64);
    match nodes {
        1 => add(&BOUNDARY_WIDTH_1, 1),
        2..=4 => add(&BOUNDARY_WIDTH_2_4, 1),
        5..=8 => add(&BOUNDARY_WIDTH_5_8, 1),
        9..=16 => add(&BOUNDARY_WIDTH_9_16, 1),
        17..=32 => add(&BOUNDARY_WIDTH_17_32, 1),
        33..=64 => add(&BOUNDARY_WIDTH_33_64, 1),
        65..=128 => add(&BOUNDARY_WIDTH_65_128, 1),
        _ => add(&BOUNDARY_WIDTH_129_PLUS, 1),
    }
}

#[inline]
pub(crate) fn count_lattice_storage_capacity(
    ends: usize,
    ends_full: usize,
    indices: usize,
    best_prev: usize,
) {
    add(&ENDS_CAPACITY_TOTAL, ends as u64);
    add(&ENDS_FULL_CAPACITY_TOTAL, ends_full as u64);
    add(&INDICES_CAPACITY_TOTAL, indices as u64);
    add(&BEST_PREV_CACHE_CAPACITY_TOTAL, best_prev as u64);
}

#[inline]
pub(crate) fn count_connection_checks(checks: u64, cost_updates: u64) {
    add(&LEFT_CONNECTION_CHECKS, checks);
    add(&COST_UPDATES, cost_updates);
}

#[inline]
pub(crate) fn count_candidate_checked() {
    add(&RIGHT_CONNECTION_CHECKS, 1);
}

#[inline]
pub(crate) fn count_rejected_candidate() {
    add(&REJECTED_CANDIDATES, 1);
}

#[inline]
pub(crate) fn count_best_prev_call() {
    add(&BEST_PREV_CALLS, 1);
}

#[inline]
pub(crate) fn count_best_prev_fast_empty() {
    add(&BEST_PREV_FAST_EMPTY, 1);
}

#[inline]
pub(crate) fn count_best_prev_fast_single() {
    add(&BEST_PREV_FAST_SINGLE, 1);
}

#[inline]
pub(crate) fn count_best_prev_direct_small() {
    add(&BEST_PREV_DIRECT_SMALL, 1);
}

#[inline]
pub(crate) fn count_lattice_prev_nodes(nodes: usize) {
    match nodes {
        0 => add(&PREV_NODES_0, 1),
        1 => add(&PREV_NODES_1, 1),
        2..=4 => add(&PREV_NODES_2_4, 1),
        5..=16 => add(&PREV_NODES_5_16, 1),
        _ => add(&PREV_NODES_17_PLUS, 1),
    }
}

#[inline]
pub(crate) fn count_left_id_cache_hit(saved_checks: usize) {
    add(&LEFT_ID_CACHE_PROBES, 1);
    add(&LEFT_ID_CACHE_HITS, 1);
    add(&LEFT_ID_CACHE_SAVED_CHECKS, saved_checks as u64);
}

#[inline]
pub(crate) fn count_left_id_cache_miss(miss_checks: usize) {
    add(&LEFT_ID_CACHE_PROBES, 1);
    add(&LEFT_ID_CACHE_MISSES, 1);
    add(&LEFT_ID_CACHE_MISS_CHECKS, miss_checks as u64);
}

#[inline]
pub(crate) fn count_best_prev_cache_push() {
    add(&BEST_PREV_CACHE_PUSHES, 1);
}

#[inline]
pub(crate) fn count_left_id_boundary(unique_left_ids: usize) {
    if unique_left_ids == 0 {
        return;
    }
    add(&LEFT_ID_BOUNDARIES, 1);
    add(&LEFT_ID_TOTAL_UNIQUE, unique_left_ids as u64);
    max(&LEFT_ID_MAX_UNIQUE_PER_BOUNDARY, unique_left_ids as u64);
}

#[inline]
pub(crate) fn count_node_vec_growth() {
    add(&NODE_VEC_GROWTHS, 1);
}

#[inline]
pub(crate) fn count_edge_vec_growth() {
    add(&EDGE_VEC_GROWTHS, 1);
}

#[inline]
pub(crate) fn count_oov_provider_call_kind(kind: OovProviderKind) {
    add(&OOV_PROVIDER_CALLS, 1);
    match kind {
        OovProviderKind::MeCab => add(&OOV_MECAB_PROVIDER_CALLS, 1),
        OovProviderKind::Simple => add(&OOV_SIMPLE_PROVIDER_CALLS, 1),
        OovProviderKind::Regex => add(&OOV_REGEX_PROVIDER_CALLS, 1),
        OovProviderKind::Other => add(&OOV_OTHER_PROVIDER_CALLS, 1),
    }
}

#[inline]
pub(crate) fn count_oov_candidates(num: usize) {
    add(&OOV_CANDIDATES_PROVIDED, num as u64);
}

#[inline]
pub(crate) fn count_oov_candidates_by_provider(kind: OovProviderKind, num: usize) {
    count_oov_candidates(num);
    match kind {
        OovProviderKind::MeCab => add(&OOV_MECAB_CANDIDATES, num as u64),
        OovProviderKind::Simple => add(&OOV_SIMPLE_CANDIDATES, num as u64),
        OovProviderKind::Regex => add(&OOV_REGEX_CANDIDATES, num as u64),
        OovProviderKind::Other => add(&OOV_OTHER_CANDIDATES, num as u64),
    }
}

#[inline]
pub(crate) fn count_oov_inserted_node() {
    add(&OOV_INSERTED_NODES, 1);
}

#[inline]
pub(crate) fn count_oov_best_path_node() {
    add(&OOV_BEST_PATH_NODES, 1);
}

#[inline]
pub(crate) fn count_oov_duplicate_candidates(num: usize) {
    add(&OOV_DUPLICATE_CANDIDATES, num as u64);
}

#[inline]
pub(crate) fn count_oov_range(candidates: usize) {
    add(&OOV_RANGES_TOTAL, 1);
    add(&OOV_RANGE_CANDIDATES_TOTAL, candidates as u64);
    max(&OOV_RANGE_MAX_CANDIDATES, candidates as u64);
    match candidates {
        0 => add(&OOV_RANGE_0, 1),
        1 => add(&OOV_RANGE_1, 1),
        2..=4 => add(&OOV_RANGE_2_4, 1),
        5..=8 => add(&OOV_RANGE_5_8, 1),
        9..=16 => add(&OOV_RANGE_9_16, 1),
        17..=23 => add(&OOV_RANGE_17_23, 1),
        _ => add(&OOV_RANGE_24_PLUS, 1),
    }
}

#[inline]
pub(crate) fn count_oov_mecab_candidate(category: CategoryType, len: usize, grouped: bool) {
    if grouped {
        add(&OOV_GROUPED_CANDIDATES, 1);
    } else {
        add(&OOV_SINGLE_CANDIDATES, 1);
    }
    match len {
        1 => add(&OOV_LENGTH_1, 1),
        2 => add(&OOV_LENGTH_2, 1),
        3 => add(&OOV_LENGTH_3, 1),
        4 => add(&OOV_LENGTH_4, 1),
        5..=8 => add(&OOV_LENGTH_5_8, 1),
        9..=16 => add(&OOV_LENGTH_9_16, 1),
        17..=32 => add(&OOV_LENGTH_17_32, 1),
        _ => add(&OOV_LENGTH_33_PLUS, 1),
    }
    if category == CategoryType::DEFAULT {
        add(&OOV_CATEGORY_DEFAULT, 1);
    } else if category == CategoryType::SPACE {
        add(&OOV_CATEGORY_SPACE, 1);
    } else if category == CategoryType::KANJI {
        add(&OOV_CATEGORY_KANJI, 1);
    } else if category == CategoryType::SYMBOL {
        add(&OOV_CATEGORY_SYMBOL, 1);
    } else if category == CategoryType::NUMERIC {
        add(&OOV_CATEGORY_NUMERIC, 1);
    } else if category == CategoryType::ALPHA {
        add(&OOV_CATEGORY_ALPHA, 1);
    } else if category == CategoryType::HIRAGANA {
        add(&OOV_CATEGORY_HIRAGANA, 1);
    } else if category == CategoryType::KATAKANA {
        add(&OOV_CATEGORY_KATAKANA, 1);
    } else if category == CategoryType::KANJINUMERIC {
        add(&OOV_CATEGORY_KANJINUMERIC, 1);
    } else if category == CategoryType::GREEK {
        add(&OOV_CATEGORY_GREEK, 1);
    } else if category == CategoryType::CYRILLIC {
        add(&OOV_CATEGORY_CYRILLIC, 1);
    } else if category == CategoryType::USER1 {
        add(&OOV_CATEGORY_USER1, 1);
    } else if category == CategoryType::USER2 {
        add(&OOV_CATEGORY_USER2, 1);
    } else if category == CategoryType::USER3 {
        add(&OOV_CATEGORY_USER3, 1);
    } else if category == CategoryType::USER4 {
        add(&OOV_CATEGORY_USER4, 1);
    } else {
        add(&OOV_CATEGORY_OTHER, 1);
    }
}

#[inline]
pub(crate) fn count_oov_suppressed_by_has_other_words() {
    add(&OOV_SUPPRESSED_BY_HAS_OTHER_WORDS, 1);
}

#[inline]
pub(crate) fn count_oov_suppressed_by_invoke_false() {
    add(&OOV_SUPPRESSED_BY_INVOKE_FALSE, 1);
}

#[inline]
pub(crate) fn count_oov_suppressed_group_by_group_false() {
    add(&OOV_SUPPRESSED_GROUP_BY_GROUP_FALSE, 1);
}

#[inline]
pub(crate) fn count_oov_buffered_provider_call() {
    add(&OOV_BUFFERED_PROVIDER_CALLS, 1);
}

#[inline]
pub(crate) fn count_oov_buffered_candidates(num: usize) {
    add(&OOV_BUFFERED_CANDIDATES, num as u64);
}

#[inline]
pub(crate) fn count_oov_temp_buffer_growth() {
    add(&OOV_TEMP_BUFFER_GROWTHS, 1);
}

#[inline]
pub(crate) fn count_oov_temp_buffer_max_len(len: usize) {
    max(&OOV_TEMP_BUFFER_MAX_LEN, len as u64);
}

#[inline]
pub(crate) fn count_oov_dominance_group() {
    add(&OOV_DOMINANCE_GROUPS, 1);
}

#[inline]
pub(crate) fn count_oov_equal_cost_ties(num: usize) {
    add(&OOV_EQUAL_COST_TIES, num as u64);
}

#[inline]
pub(crate) fn count_oov_unique_after_dominance(num: usize) {
    add(&OOV_UNIQUE_AFTER_DOMINANCE, num as u64);
}

#[inline]
pub(crate) fn count_oov_strict_dominated_candidate(category: CategoryType, len: usize) {
    add(&OOV_STRICT_DOMINATED_CANDIDATES, 1);
    match len {
        1 => add(&OOV_DOMINATED_LENGTH_1, 1),
        2 => add(&OOV_DOMINATED_LENGTH_2, 1),
        3 => add(&OOV_DOMINATED_LENGTH_3, 1),
        4 => add(&OOV_DOMINATED_LENGTH_4, 1),
        5..=8 => add(&OOV_DOMINATED_LENGTH_5_8, 1),
        9..=16 => add(&OOV_DOMINATED_LENGTH_9_16, 1),
        17..=32 => add(&OOV_DOMINATED_LENGTH_17_32, 1),
        _ => add(&OOV_DOMINATED_LENGTH_33_PLUS, 1),
    }
    if category == CategoryType::DEFAULT {
        add(&OOV_DOMINATED_CATEGORY_DEFAULT, 1);
    } else if category == CategoryType::SPACE {
        add(&OOV_DOMINATED_CATEGORY_SPACE, 1);
    } else if category == CategoryType::KANJI {
        add(&OOV_DOMINATED_CATEGORY_KANJI, 1);
    } else if category == CategoryType::SYMBOL {
        add(&OOV_DOMINATED_CATEGORY_SYMBOL, 1);
    } else if category == CategoryType::NUMERIC {
        add(&OOV_DOMINATED_CATEGORY_NUMERIC, 1);
    } else if category == CategoryType::ALPHA {
        add(&OOV_DOMINATED_CATEGORY_ALPHA, 1);
    } else if category == CategoryType::HIRAGANA {
        add(&OOV_DOMINATED_CATEGORY_HIRAGANA, 1);
    } else if category == CategoryType::KATAKANA {
        add(&OOV_DOMINATED_CATEGORY_KATAKANA, 1);
    } else if category == CategoryType::KANJINUMERIC {
        add(&OOV_DOMINATED_CATEGORY_KANJINUMERIC, 1);
    } else if category == CategoryType::GREEK {
        add(&OOV_DOMINATED_CATEGORY_GREEK, 1);
    } else if category == CategoryType::CYRILLIC {
        add(&OOV_DOMINATED_CATEGORY_CYRILLIC, 1);
    } else if category == CategoryType::USER1 {
        add(&OOV_DOMINATED_CATEGORY_USER1, 1);
    } else if category == CategoryType::USER2 {
        add(&OOV_DOMINATED_CATEGORY_USER2, 1);
    } else if category == CategoryType::USER3 {
        add(&OOV_DOMINATED_CATEGORY_USER3, 1);
    } else if category == CategoryType::USER4 {
        add(&OOV_DOMINATED_CATEGORY_USER4, 1);
    } else {
        add(&OOV_DOMINATED_CATEGORY_OTHER, 1);
    }
}

#[inline]
pub(crate) fn count_token_position_reachable() {
    add(&POSITIONS_TOTAL, 1);
    add(&POSITIONS_REACHABLE, 1);
}

#[inline]
pub(crate) fn count_token_position_unreachable_skipped() {
    add(&POSITIONS_TOTAL, 1);
    add(&POSITIONS_UNREACHABLE_SKIPPED, 1);
}

#[inline]
pub(crate) fn count_lattice_direct_candidate() {
    add(&DIRECT_INSERTED_CANDIDATES, 1);
}
