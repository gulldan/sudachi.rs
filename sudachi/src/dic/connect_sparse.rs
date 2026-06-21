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

//! Additive + sparse-residual representation of the connection matrix.
//!
//! A MeCab/Sudachi connection matrix `M(left, right)` is a dense `num_left ×
//! num_right` table of i16 CRF connection costs. Empirically (see `matrix_l1_gsd`
//! on GSD) it is well approximated by an additive base `A[left] + B[right]` plus a
//! sparse set of large residuals — most cells are noise around the additive part.
//!
//! This type stores `A`, `B`, and the residuals `R(left,right) = M − A − B` whose
//! magnitude is `>= lambda`, in CSR form keyed by `right` (the row the hot Viterbi
//! loop scans). `cost()` returns `A[left] + B[right] + R(left,right)`.
//!
//! - `lambda == 0` keeps every residual → output is **byte-identical** to the
//!   dense matrix (verified by `cost_matches_dense_at_lambda0`).
//! - `lambda > 0` drops small residuals → smaller working set, approximate cost.
//!   This is the structure eiennohito's "simplify / L1-regularize the matrix"
//!   suggestion produces; whether the smaller footprint outweighs the per-access
//!   residual lookup is a runtime measurement, not an assumption.

/// Connection matrix stored as an additive base plus sparse residuals.
pub struct SparseConnectionMatrix {
    num_left: usize,
    num_right: usize,
    /// Additive base, one entry per `left` id.
    a: Vec<i32>,
    /// Additive base, one entry per `right` id.
    b: Vec<i32>,
    /// CSR residuals keyed by `right`: row `r` is `cols[row_offsets[r]..row_offsets[r+1]]`
    /// (sorted `left` ids) with parallel `vals`.
    row_offsets: Vec<u32>,
    cols: Vec<u16>,
    vals: Vec<i16>,
}

impl SparseConnectionMatrix {
    /// Build from the dense matrix data (`data[right * num_left + left]`), keeping
    /// only residuals with `|M − A − B| >= lambda`.
    pub fn from_dense(data: &[i16], num_left: usize, num_right: usize, lambda: i32) -> Self {
        assert_eq!(data.len(), num_left * num_right);
        let m = |left: usize, right: usize| -> i32 { data[right * num_left + left] as i32 };

        // A[left] = mean over right of M(left, right).
        let mut a = vec![0i32; num_left];
        for (left, a_l) in a.iter_mut().enumerate() {
            let mut sum = 0i64;
            for right in 0..num_right {
                sum += m(left, right) as i64;
            }
            *a_l = (sum / num_right.max(1) as i64) as i32;
        }
        // B[right] = mean over left of (M(left, right) − A[left]).
        let mut b = vec![0i32; num_right];
        for (right, b_r) in b.iter_mut().enumerate() {
            let mut sum = 0i64;
            for left in 0..num_left {
                sum += (m(left, right) - a[left]) as i64;
            }
            *b_r = (sum / num_left.max(1) as i64) as i32;
        }

        // Residuals R(left, right) = M − A − B, kept when |R| >= lambda, CSR by right.
        let mut row_offsets = Vec::with_capacity(num_right + 1);
        let mut cols: Vec<u16> = Vec::new();
        let mut vals: Vec<i16> = Vec::new();
        row_offsets.push(0u32);
        for right in 0..num_right {
            for left in 0..num_left {
                let r = m(left, right) - a[left] - b[right];
                if r.abs() >= lambda {
                    cols.push(left as u16);
                    vals.push(r as i16);
                }
            }
            row_offsets.push(cols.len() as u32);
        }

        SparseConnectionMatrix {
            num_left,
            num_right,
            a,
            b,
            row_offsets,
            cols,
            vals,
        }
    }

    /// `A[left] + B[right] + R(left, right)` as i16 (the residual is 0 when absent).
    #[inline(always)]
    pub fn cost(&self, left: u16, right: u16) -> i16 {
        let base = self.a[left as usize] + self.b[right as usize];
        let lo = self.row_offsets[right as usize] as usize;
        let hi = self.row_offsets[right as usize + 1] as usize;
        let row = &self.cols[lo..hi];
        let resid = match row.binary_search(&left) {
            Ok(i) => self.vals[lo + i] as i32,
            Err(_) => 0,
        };
        (base + resid) as i16
    }

    /// Heap bytes of the sparse representation (for footprint comparison).
    pub fn heap_bytes(&self) -> usize {
        self.a.len() * 4
            + self.b.len() * 4
            + self.row_offsets.len() * 4
            + self.cols.len() * 2
            + self.vals.len() * 2
    }

    /// Number of stored (non-zeroed) residuals.
    pub fn nnz(&self) -> usize {
        self.cols.len()
    }

    pub fn num_left(&self) -> usize {
        self.num_left
    }
    pub fn num_right(&self) -> usize {
        self.num_right
    }
}

/// Connection matrix approximated by co-clustering left/right ids into `k`
/// classes each (deterministic k-means over sampled cost profiles) and storing a
/// small dense `kl × kr` block of class-pair means. `cost()` is a single indexed
/// load into a table that fits L1 — but it is **lossy**: it merges connection
/// ids, so output is not byte-identical (quality is measured, not assumed).
pub struct BlockClassMatrix {
    kr: usize,
    cll: Vec<u16>, // left id -> class
    clr: Vec<u16>, // right id -> class
    block: Vec<i16>, // kl x kr, row-major (kr columns)
}

impl BlockClassMatrix {
    /// Build from the dense matrix (`data[right * num_left + left]`). `sample`
    /// cost columns/rows are used as the clustering profile to keep k-means cheap.
    pub fn from_dense(
        data: &[i16],
        num_left: usize,
        num_right: usize,
        k: usize,
        sample: usize,
        iters: usize,
    ) -> Self {
        let m = |left: usize, right: usize| -> i16 { data[right * num_left + left] };
        let sample = sample.max(1);
        let sampled_l: Vec<usize> = (0..num_left)
            .step_by((num_left / sample).max(1))
            .take(sample)
            .collect();
        let sampled_r: Vec<usize> = (0..num_right)
            .step_by((num_right / sample).max(1))
            .take(sample)
            .collect();

        // Cluster right ids by their cost profile over the sampled left ids.
        let rpoints: Vec<Vec<f32>> = (0..num_right)
            .map(|r| sampled_l.iter().map(|&l| m(l, r) as f32).collect())
            .collect();
        let clr = kmeans(&rpoints, k, iters);
        // Cluster left ids by their cost profile over the sampled right ids.
        let lpoints: Vec<Vec<f32>> = (0..num_left)
            .map(|l| sampled_r.iter().map(|&r| m(l, r) as f32).collect())
            .collect();
        let cll = kmeans(&lpoints, k, iters);

        let kl = k.min(num_left);
        let kr = k.min(num_right);
        let mut bsum = vec![0i64; kl * kr];
        let mut bcnt = vec![0u64; kl * kr];
        for r in 0..num_right {
            let b = clr[r] as usize;
            for l in 0..num_left {
                let a = cll[l] as usize;
                bsum[a * kr + b] += m(l, r) as i64;
                bcnt[a * kr + b] += 1;
            }
        }
        let block: Vec<i16> = (0..kl * kr)
            .map(|i| {
                if bcnt[i] > 0 {
                    (bsum[i] / bcnt[i] as i64) as i16
                } else {
                    0
                }
            })
            .collect();

        BlockClassMatrix { kr, cll, clr, block }
    }

    #[inline(always)]
    pub fn cost(&self, left: u16, right: u16) -> i16 {
        let a = self.cll[left as usize] as usize;
        let b = self.clr[right as usize] as usize;
        self.block[a * self.kr + b]
    }

    pub fn heap_bytes(&self) -> usize {
        self.cll.len() * 2 + self.clr.len() * 2 + self.block.len() * 2
    }
}

/// Deterministic k-means (centroids seeded by evenly-spaced points; no RNG).
/// Returns the class assignment for each point.
fn kmeans(points: &[Vec<f32>], k: usize, iters: usize) -> Vec<u16> {
    let n = points.len();
    if n == 0 {
        return Vec::new();
    }
    let dim = points[0].len();
    let k = k.min(n).max(1);
    let mut cent: Vec<Vec<f32>> = (0..k).map(|i| points[i * n / k].clone()).collect();
    let mut assign = vec![0u16; n];
    for _ in 0..iters {
        for (pi, p) in points.iter().enumerate() {
            let mut best = 0usize;
            let mut bestd = f32::MAX;
            for (ci, c) in cent.iter().enumerate() {
                let mut d = 0f32;
                for j in 0..dim {
                    let diff = p[j] - c[j];
                    d += diff * diff;
                    if d >= bestd {
                        break;
                    }
                }
                if d < bestd {
                    bestd = d;
                    best = ci;
                }
            }
            assign[pi] = best as u16;
        }
        let mut sums = vec![vec![0f64; dim]; k];
        let mut cnts = vec![0u64; k];
        for (pi, p) in points.iter().enumerate() {
            let a = assign[pi] as usize;
            cnts[a] += 1;
            for j in 0..dim {
                sums[a][j] += p[j] as f64;
            }
        }
        for ci in 0..k {
            if cnts[ci] > 0 {
                for j in 0..dim {
                    cent[ci][j] = (sums[ci][j] / cnts[ci] as f64) as f32;
                }
            }
        }
    }
    assign
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dense(num_left: usize, num_right: usize, seed: i64) -> Vec<i16> {
        // Deterministic pseudo-random i16 matrix (no Math.random; pure arithmetic).
        let mut v = Vec::with_capacity(num_left * num_right);
        let mut s = seed as i64;
        for _ in 0..num_left * num_right {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            v.push((s >> 33) as i16);
        }
        v
    }

    #[test]
    fn cost_matches_dense_at_lambda0() {
        let (nl, nr) = (37, 53);
        let data = dense(nl, nr, 12345);
        let sparse = SparseConnectionMatrix::from_dense(&data, nl, nr, 0);
        for right in 0..nr {
            for left in 0..nl {
                let want = data[right * nl + left];
                let got = sparse.cost(left as u16, right as u16);
                assert_eq!(got, want, "mismatch at left={left} right={right}");
            }
        }
    }

    #[test]
    fn lambda_zeroes_small_residuals_only() {
        let (nl, nr) = (37, 53);
        let data = dense(nl, nr, 999);
        let full = SparseConnectionMatrix::from_dense(&data, nl, nr, 0);
        let sparse = SparseConnectionMatrix::from_dense(&data, nl, nr, 4000);
        // Thresholding can only drop residuals, never add them.
        assert!(sparse.nnz() <= full.nnz());
        // Where the thresholded matrix differs from the dense, the gap is < lambda.
        for right in 0..nr {
            for left in 0..nl {
                let dense_v = data[right * nl + left] as i32;
                let approx = sparse.cost(left as u16, right as u16) as i32;
                assert!((dense_v - approx).abs() < 4000 + 1);
            }
        }
    }
}
