/*
 *  Copyright (c) 2021-2026 Works Applications Co., Ltd.
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

use nom::number::complete::le_i16;

use crate::error::{SudachiError, SudachiResult};
use crate::util::cow_array::CowArray;

pub struct ConnectionMatrix<'a> {
    data: CowArray<'a, i16>,
    num_left: usize,
    num_right: usize,
    /// Clean simplified-matrix override (issue-117). `None` = the dense matrix as
    /// loaded. Built by [`ConnectionMatrix::sparsify`] / [`ConnectionMatrix::blockify`].
    simplified: Option<MatrixOverride>,
}

/// A clean drop-in replacement for the dense connection matrix (issue-117 matrix
/// simplification). Each variant answers `cost(left, right)`.
enum MatrixOverride {
    /// Additive base + sparse residuals; byte-identical at lambda=0.
    Sparse(crate::dic::connect_sparse::SparseConnectionMatrix),
    /// Co-clustered class block (small dense, L1-resident); lossy.
    Block(crate::dic::connect_sparse::BlockClassMatrix),
}

// ----------------------------------------------------------------------------
// PROBE (not for upstream): a process-global, runtime-toggleable index mask for
// the connection-matrix load. `cost()` does `index & PROBE_MASK` before the
// load; `usize::MAX` is a no-op (still emits the AND, for fairness). Confining
// the index to a small power-of-two footprint forces every matrix access into
// L1/L2 while keeping the *exact same instruction stream* and the *exact same
// number of loads* — a causal test of "is the matrix memory-latency bound?"
// that needs no profiler attribution. Being a global (read with a relaxed
// atomic = a plain load on aarch64) lets a benchmark flip it per trial so a
// single process can interleave baseline vs masked with shared thermal/cache
// state, killing cross-process drift.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
pub static PROBE_MASK: AtomicUsize = AtomicUsize::new(usize::MAX);
static PROBE_ENV_INIT: std::sync::Once = std::sync::Once::new();

// PROBE: working-set recorder. When enabled, every real (unmasked) matrix index
// is recorded so we can measure how many distinct cells / cache lines / bytes
// natural text actually touches — the figure that decides which cache level the
// matrix lives in on a given CPU. One untimed pass only.
pub static PROBE_RECORD: AtomicBool = AtomicBool::new(false);
pub static PROBE_TOUCHED: std::sync::Mutex<Option<std::collections::HashSet<u32>>> =
    std::sync::Mutex::new(None);

pub fn probe_record_begin() {
    *PROBE_TOUCHED.lock().unwrap() = Some(std::collections::HashSet::new());
    PROBE_RECORD.store(true, Ordering::Relaxed);
}

/// Stops recording and returns the set of distinct cell indices touched.
pub fn probe_record_end() -> Vec<u32> {
    PROBE_RECORD.store(false, Ordering::Relaxed);
    PROBE_TOUCHED
        .lock()
        .unwrap()
        .take()
        .map(|s| s.into_iter().collect())
        .unwrap_or_default()
}

/// Set the probe mask at runtime. `entries` is a power-of-two footprint in i16
/// entries (e.g. 8192 = 16 KiB); `usize::MAX` restores the full matrix.
pub fn set_probe_mask(entries: usize) {
    let mask = if entries == usize::MAX {
        usize::MAX
    } else if entries.is_power_of_two() {
        entries - 1
    } else {
        usize::MAX
    };
    PROBE_MASK.store(mask, Ordering::Relaxed);
}

/// PROBE: when true, `cost()` returns arithmetic on the index instead of loading
/// from the matrix — reproduces the study note's `nomat` probe.
pub static PROBE_NOMAT: AtomicBool = AtomicBool::new(false);
pub fn set_probe_nomat(b: bool) {
    PROBE_NOMAT.store(b, Ordering::Relaxed);
}

// PROBE: connection-class approximation. Emulates "a simpler model with fewer
// effective connection classes" (what merging redundant IDs / a sparser CRF
// would yield): each left/right id is mapped to a class, and cost is read from
// a small KL x KR block matrix. If KL*KR fits L1, this is the structure that
// would recover the measured ~7-9%. Lets us measure how much *output* changes
// at that compression — the missing half of "relearn the matrix".
pub struct ApproxModel {
    pub cll: Vec<u16>,   // left id -> class, len = num_left
    pub clr: Vec<u16>,   // right id -> class, len = num_right
    pub block: Vec<i16>, // KL x KR, row-major (kr columns)
    pub kr: usize,
}
pub static PROBE_APPROX: AtomicBool = AtomicBool::new(false);
static APPROX_MODEL: std::sync::OnceLock<ApproxModel> = std::sync::OnceLock::new();
pub fn install_approx(m: ApproxModel) {
    let _ = APPROX_MODEL.set(m);
    PROBE_APPROX.store(true, Ordering::Relaxed);
}

// PROBE: full-matrix replacement. Lets an experiment swap in an arbitrary
// approximated matrix (e.g. a low-rank reconstruction) and measure how much the
// tokenization OUTPUT changes. Same layout as the real matrix.
pub static PROBE_REPLACE: AtomicBool = AtomicBool::new(false);
static REPLACE_MATRIX: std::sync::OnceLock<Vec<i16>> = std::sync::OnceLock::new();
pub fn install_replace(m: Vec<i16>) {
    let _ = REPLACE_MATRIX.set(m);
    PROBE_REPLACE.store(true, Ordering::Relaxed);
}

fn init_probe_from_env(size: usize) {
    PROBE_ENV_INIT.call_once(|| {
        if let Some(entries) = std::env::var("SUDACHI_MATRIX_MASK")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|e| e.is_power_of_two() && *e <= size)
        {
            PROBE_MASK.store(entries - 1, Ordering::Relaxed);
            eprintln!(
                "# MATRIX PROBE active from env: footprint={} entries / {} bytes",
                entries,
                entries * 2
            );
        }
    });
}

impl<'a> ConnectionMatrix<'a> {
    pub fn from_bytes(buf: &'a [u8]) -> SudachiResult<ConnectionMatrix<'a>> {
        let (rest, (num_left, num_right)) = nom::sequence::tuple((le_i16, le_i16))(buf)?;
        Self::from_offset_size(rest, 0, num_left as usize, num_right as usize)
    }

    pub fn from_offset_size(
        data: &'a [u8],
        offset: usize,
        num_left: usize,
        num_right: usize,
    ) -> SudachiResult<ConnectionMatrix<'a>> {
        let size = num_left * num_right;
        let end = offset + size * std::mem::size_of::<i16>();
        if end > data.len() {
            return Err(SudachiError::InvalidDictionaryGrammar.with_context("connection matrix"));
        }

        init_probe_from_env(size);
        Ok(ConnectionMatrix {
            data: CowArray::from_bytes(data, offset, size),
            num_left,
            num_right,
            simplified: None,
        })
    }

    #[inline(always)]
    fn index(&self, left: u16, right: u16) -> usize {
        let uleft = left as usize;
        let uright = right as usize;
        debug_assert!(
            uleft < self.num_left,
            "left id {} is out of range (num_left={})",
            uleft,
            self.num_left
        );
        debug_assert!(
            uright < self.num_right,
            "right id {} is out of range (num_right={})",
            uright,
            self.num_right
        );
        let index = uright * self.num_left + uleft;
        debug_assert!(index < self.data.len());
        index
    }

    /// Gets the value of the connection matrix
    ///
    /// It is performance critical that this function
    /// 1. Has no branches
    /// 2. Is inlined to the caller
    ///
    /// This is UB if index is out of bounds, but that can't happen
    /// except in the case if the binary dictionary was tampered with.
    /// It is OK to make usage of tampered binary dictionaries UB.
    #[inline(always)]
    pub fn cost(&self, left: u16, right: u16) -> i16 {
        match &self.simplified {
            Some(MatrixOverride::Sparse(s)) => return s.cost(left, right),
            Some(MatrixOverride::Block(b)) => return b.cost(left, right),
            None => {}
        }
        if PROBE_REPLACE.load(Ordering::Relaxed) {
            if let Some(m) = REPLACE_MATRIX.get() {
                return unsafe { *m.get_unchecked(self.index(left, right)) };
            }
        }
        if PROBE_APPROX.load(Ordering::Relaxed) {
            if let Some(m) = APPROX_MODEL.get() {
                let a = m.cll[left as usize] as usize;
                let b = m.clr[right as usize] as usize;
                return m.block[a * m.kr + b];
            }
        }
        // PROBE: `& PROBE_MASK` is present in every run (no-op when the mask is
        // usize::MAX) so the instruction stream is identical; only the load's
        // memory footprint changes. Relaxed load = a plain LDR on aarch64.
        let raw = self.index(left, right);
        if PROBE_RECORD.load(Ordering::Relaxed) {
            if let Ok(mut g) = PROBE_TOUCHED.lock() {
                if let Some(set) = g.as_mut() {
                    set.insert(raw as u32);
                }
            }
        }
        let index = raw & PROBE_MASK.load(Ordering::Relaxed);
        if PROBE_NOMAT.load(Ordering::Relaxed) {
            return index as i16;
        }
        *unsafe { self.data.get_unchecked(index) }
    }

    pub fn update(&mut self, left: u16, right: u16, value: i16) {
        let index = self.index(left, right);
        self.data.set(index, value);
    }

    /// Replace the dense matrix with an additive base `A[left] + B[right]` plus
    /// sparse residuals keeping `|M − A − B| >= lambda` (issue-117 matrix
    /// simplification, eiennohito's "simplify the CRF" direction). `lambda == 0`
    /// is byte-identical to the dense matrix; larger lambda trades a smaller
    /// working set for a per-access residual lookup. Returns `(nnz, heap_bytes)`.
    pub fn sparsify(&mut self, lambda: i32) -> (usize, usize) {
        let s = crate::dic::connect_sparse::SparseConnectionMatrix::from_dense(
            &self.data,
            self.num_left,
            self.num_right,
            lambda,
        );
        let info = (s.nnz(), s.heap_bytes());
        self.simplified = Some(MatrixOverride::Sparse(s));
        info
    }

    /// Replace the dense matrix with a co-clustered class block (small dense,
    /// L1-resident, lossy) — the alternative speed candidate to [`Self::sparsify`].
    /// `k` classes per side, `sample` profile columns, `iters` k-means passes.
    /// Returns `heap_bytes` of the block form.
    pub fn blockify(&mut self, k: usize, sample: usize, iters: usize) -> usize {
        let b = crate::dic::connect_sparse::BlockClassMatrix::from_dense(
            &self.data,
            self.num_left,
            self.num_right,
            k,
            sample,
            iters,
        );
        let bytes = b.heap_bytes();
        self.simplified = Some(MatrixOverride::Block(b));
        bytes
    }

    /// Returns maximum number of left connection ID
    pub fn num_left(&self) -> usize {
        self.num_left
    }

    /// Returns maximum number of right connection ID
    pub fn num_right(&self) -> usize {
        self.num_right
    }
}
