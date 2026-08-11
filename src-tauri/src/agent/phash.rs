//! Perceptual image hashing (Perception, `PHS-1`): a 64-bit dHash — downscale
//! to 9×8 greyscale, then one bit per pixel for whether it's darker than its
//! right neighbour. Two images that look alike hash to a small Hamming
//! distance even under recompression, resizing, or a format change; two
//! images that don't, don't.

use std::path::Path;

use image::imageops::FilterType;

/// Thresholds from heapchat's calibration on ~2.8k real hashes (`PHS-2`).
pub const IDENTICAL_MAX: u32 = 2;
pub const NEAR_MAX: u32 = 10;
pub const RELATED_MAX: u32 = 23;

/// `None` on anything that isn't decodable as an image — a corrupt file or a
/// misnamed extension, either way nothing to hash.
pub fn dhash(path: &Path) -> Option<u64> {
    let img = image::open(path).ok()?;
    // 9 wide so each of the 8 output columns has a right neighbour to compare
    // against — the classic dHash shape.
    let small = img.resize_exact(9, 8, FilterType::Triangle).into_luma8();
    let mut hash: u64 = 0;
    let mut bit = 0u32;
    for y in 0..8 {
        for x in 0..8 {
            let left = small.get_pixel(x, y)[0];
            let right = small.get_pixel(x + 1, y)[0];
            if left > right {
                hash |= 1 << bit;
            }
            bit += 1;
        }
    }
    Some(hash)
}

/// Bits that differ between two hashes — 0 means pixel-identical downscales.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn write_png(path: &Path, fill: [u8; 3]) {
        let img = ImageBuffer::from_fn(32, 32, |_, _| Rgb(fill));
        img.save(path).unwrap();
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("poiesis_phash_{name}_{}.png", uuid::Uuid::new_v4()))
    }

    #[test]
    fn identical_images_hash_to_zero_distance() {
        let a = scratch("a");
        let b = scratch("b");
        write_png(&a, [200, 40, 40]);
        write_png(&b, [200, 40, 40]);
        let ha = dhash(&a).unwrap();
        let hb = dhash(&b).unwrap();
        assert_eq!(hamming(ha, hb), 0);
        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

    #[test]
    fn a_flat_image_and_a_checkerboard_hash_far_apart() {
        // A monotonic gradient is the wrong counter-example here: dHash only
        // encodes the *sign* of each row-wise step, so a flat fill (every
        // step equal) and a smoothly increasing gradient (every step the
        // same sign) can legitimately hash identically. A checkerboard's
        // steps flip sign constantly, which a flat fill's never do.
        let a = scratch("flat");
        let b = scratch("checker");
        write_png(&a, [128, 128, 128]);
        let checker = ImageBuffer::from_fn(32, 32, |x, y| {
            let on = (x / 4 + y / 4) % 2 == 0;
            Rgb([if on { 250u8 } else { 5u8 }; 3])
        });
        checker.save(&b).unwrap();
        let ha = dhash(&a).unwrap();
        let hb = dhash(&b).unwrap();
        assert!(hamming(ha, hb) > NEAR_MAX, "a flat fill and a checkerboard should not read as near-duplicates");
        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

    #[test]
    fn a_missing_or_undecodable_file_hashes_to_none() {
        let f = std::env::temp_dir().join(format!("poiesis_phash_bad_{}.png", uuid::Uuid::new_v4()));
        std::fs::write(&f, b"not a png").unwrap();
        assert!(dhash(&f).is_none());
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn hamming_of_a_hash_with_itself_is_zero() {
        assert_eq!(hamming(0xABCD, 0xABCD), 0);
    }

    #[test]
    fn hamming_counts_differing_bits() {
        assert_eq!(hamming(0b0000, 0b1111), 4);
    }
}
