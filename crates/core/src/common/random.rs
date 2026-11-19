//! Keyed random numbers: each number is a function of a label saying what it
//! is for, never of how many numbers were drawn before it.
//!
//! A label is a list of 64-bit parts, absorbed in order with SplitMix64's
//! mixer. The label's hash starts a SplitMix64 sequence: a uniform number is
//! its first output, a normal number its first two (Box-Muller). A name enters
//! a label as its 64-bit FNV-1a hash, a signed integer as its two's-complement
//! bits.

/// SplitMix64's increment, the golden ratio in 64 bits.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// SplitMix64's output function: every input bit affects every output bit.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Adds one part to a label's hash.
pub(crate) fn absorb(hash: u64, part: u64) -> u64 {
    mix(hash.wrapping_add(GAMMA) ^ part)
}

/// The hash of a whole label, parts in order.
pub(crate) fn label(parts: &[u64]) -> u64 {
    parts.iter().fold(0, |hash, &part| absorb(hash, part))
}

/// A name as a label part: its UTF-8 bytes hashed with 64-bit FNV-1a.
pub(crate) fn name(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The `n`th output (from 1) of the SplitMix64 sequence a label starts,
/// as a number strictly inside (0, 1): 52 random bits, centred in their bin.
fn output(label: u64, n: u64) -> f64 {
    let bits = mix(label.wrapping_add(GAMMA.wrapping_mul(n)));
    ((bits >> 12) as f64 + 0.5) / 4_503_599_627_370_496.0
}

/// The label's uniform number, strictly inside (0, 1).
pub(crate) fn uniform(label: u64) -> f64 {
    output(label, 1)
}

/// The label's normal number (Box-Muller, cosine branch).
pub(crate) fn normal(label: u64, mean: f64, stddev: f64) -> f64 {
    let radius = (-2.0 * output(label, 1).ln()).sqrt();
    let angle_rad = std::f64::consts::TAU * output(label, 2);
    mean + stddev * radius * angle_rad.cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_fixes_its_numbers_and_every_part_and_its_order_matter() {
        let base = label(&[7, 1, 2]);
        assert_eq!(base, label(&[7, 1, 2]));
        for other in [
            label(&[7, 1, 3]),
            label(&[7, 2, 1]),
            label(&[7, 1]),
            label(&[7, 1, 2, 0]),
        ] {
            assert_ne!(base, other);
        }
        assert_eq!(uniform(base), uniform(label(&[7, 1, 2])));
    }

    /// Values from an independent implementation of the scheme (Python), so a
    /// change to any step shows; external verification reproduces
    /// the same numbers.
    #[test]
    fn known_values() {
        assert_eq!(name(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(name("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(label(&[]), 0);
        assert_eq!(label(&[0]), 0xe220_a839_7b1d_cdaf);
        let draw = label(&[12345, 1, name("NoisyState:0"), 3, 0, 2]);
        assert_eq!(draw, 0xcd81_dea7_8f76_ea4a);
        assert_eq!(uniform(draw), 0.7514320376920504);
        // ln and cos may differ in the last bit between math libraries.
        assert!((normal(draw, 0.0, 1.0) - 0.20649950472264123).abs() < 1e-15);
    }

    #[test]
    fn uniform_and_normal_have_their_moments() {
        let count = 100_000;
        let (mut sum_u, mut sum_x, mut sum_xx) = (0.0, 0.0, 0.0);
        for index in 0..count {
            let key = label(&[1, index]);
            let u = uniform(key);
            assert!(u > 0.0 && u < 1.0);
            sum_u += u;
            let x = normal(key, 2.0, 0.5);
            sum_x += x;
            sum_xx += x * x;
        }
        let n = count as f64;
        let mean = sum_x / n;
        assert!((sum_u / n - 0.5).abs() < 0.005);
        assert!((mean - 2.0).abs() < 0.005);
        assert!((sum_xx / n - mean * mean - 0.25).abs() < 0.005);
    }
}
