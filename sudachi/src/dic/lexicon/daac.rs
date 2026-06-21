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

use daachorse::CharwiseDoubleArrayAhoCorasick;

use crate::dic::description::DescriptionError;
use crate::prelude::*;

const MAGIC: &[u8; 8] = b"SDDAAC01";

/// Serialized charwise double-array Aho-Corasick index over dictionary
/// index-forms. Values are WordIdTable offsets, matching the trie values.
pub(crate) struct CharwiseDaacIndex {
    automaton: CharwiseDoubleArrayAhoCorasick<u32>,
}

impl CharwiseDaacIndex {
    pub(crate) fn from_bytes(bytes: &[u8]) -> SudachiResult<Self> {
        if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
            return Err(DescriptionError::CannotParse.into());
        }

        // daachorse 1.0.x only exposes unchecked deserialization. The block is
        // emitted by Sudachi's dictionary builder, and old dictionaries simply
        // do not have the optional block.
        let (automaton, rest) = unsafe {
            CharwiseDoubleArrayAhoCorasick::<u32>::deserialize_unchecked(&bytes[MAGIC.len()..])
        };
        if !rest.is_empty() {
            return Err(DescriptionError::CannotParse.into());
        }

        Ok(Self { automaton })
    }

    #[inline]
    pub(crate) fn find_overlapping_iter<'a>(
        &'a self,
        input: &'a str,
    ) -> impl Iterator<Item = daachorse::Match<u32>> + 'a {
        self.automaton.find_overlapping_iter(input)
    }
}

pub(crate) fn serialize_charwise_daac(automaton: &CharwiseDoubleArrayAhoCorasick<u32>) -> Vec<u8> {
    let serialized = automaton.serialize();
    let mut out = Vec::with_capacity(MAGIC.len() + serialized.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&serialized);
    out
}
