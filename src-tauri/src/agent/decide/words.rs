//! `SEE-4`: how sure a decision is, in words and not in a number. The number stays
//! in the hover and the detail card (decision 15). `src/lib/decisions.ts` has the same
//! rule as `sureWord`; the two are kept equal by their tests.

/// What a surface says about a decision that acted or was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sure {
    /// Said plainly.
    Sure,
    /// Said with "probably" or "may".
    Probably,
}

/// A calibrated probability from here is sure.
pub const SURE_CALIBRATED: f32 = 0.85;
/// An uncalibrated one has to be this high: nothing has checked what it means.
pub const SURE_UNCALIBRATED: f32 = 0.9;
/// Below this a decision did not act, so there is nothing to say.
pub const PROBABLY_FROM: f32 = 0.6;

/// Sure, probably, or nothing worth saying.
pub fn sure_word(p: f32, calibrated: bool) -> Option<Sure> {
    if p >= if calibrated { SURE_CALIBRATED } else { SURE_UNCALIBRATED } {
        Some(Sure::Sure)
    } else if p >= PROBABLY_FROM {
        Some(Sure::Probably)
    } else {
        None
    }
}

/// `plain` when sure, `hedged` otherwise. A surface that exists only because a
/// decision acted still says something below the floor, so it hedges.
pub fn say<'a>(p: f32, calibrated: bool, plain: &'a str, hedged: &'a str) -> &'a str {
    match sure_word(p, calibrated) {
        Some(Sure::Sure) => plain,
        _ => hedged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SEE-T-8`: calibrated, sure from 0.85; uncalibrated, only from 0.9; both
    /// say "probably" from 0.6; below that, nothing.
    #[test]
    fn a_calibrated_value_is_sure_from_085_and_an_uncalibrated_one_from_09() {
        assert_eq!(sure_word(0.95, true), Some(Sure::Sure));
        assert_eq!(sure_word(0.85, true), Some(Sure::Sure));
        assert_eq!(sure_word(0.84, true), Some(Sure::Probably));
        assert_eq!(sure_word(0.88, false), Some(Sure::Probably), "an uncalibrated value under 0.9 is probably");
        assert_eq!(sure_word(0.9, false), Some(Sure::Sure));
        assert_eq!(sure_word(0.6, false), Some(Sure::Probably));
        assert_eq!(sure_word(0.6, true), Some(Sure::Probably));
        assert_eq!(sure_word(0.59, true), None);
        assert_eq!(sure_word(0.0, false), None);
    }

    #[test]
    fn a_surface_that_exists_anyway_hedges_below_sure() {
        assert_eq!(say(0.95, false, "is", "may be"), "is");
        assert_eq!(say(0.7, false, "is", "may be"), "may be");
        assert_eq!(say(0.2, true, "is", "may be"), "may be");
    }
}
