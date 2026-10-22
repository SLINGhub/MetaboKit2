//! Peak integration kernel adapted from MRMhub INTEGRATOR (SLING), step 3
//! `get_auc.rs`: trapezoidal area above a baseline, apex, height and FWHM.
//!
//! Scans are included when `start <= rt < end`, the DIA engine's own rule, and
//! `Baseline::None` uses the engine's area expression unchanged, so the
//! engine's windows reproduce its reported areas exactly.
use crate::integration::{Baseline, Window};
use serde::Serialize;
use std::cmp::Ordering;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peak {
    pub area: f32,
    /// Apex intensity, not baseline-subtracted (as in MRMhub).
    pub height: f32,
    pub apex: Option<f32>,
    pub fwhm: Option<f32>,
    /// Baseline intensity at the window's first and last scan.
    pub baseline: (f32, f32),
    pub scans: usize,
}

fn slice(trace: &[(f32, f32)], lo: f32, hi: f32) -> &[(f32, f32)] {
    let a = trace.partition_point(|p| p.0 < lo);
    let b = a + trace[a..].partition_point(|p| p.0 < hi);
    &trace[a..b]
}

/// Integrates `trace` over `window`. For the percentile baseline the trace
/// must also cover one window width either side.
pub fn integrate(trace: &[(f32, f32)], window: Window, baseline: Baseline) -> Peak {
    let peak = slice(trace, window.start, window.end);
    let (area, baseline) = match baseline {
        Baseline::None => (engine_auc(peak), (0.0, 0.0)),
        Baseline::Percentile => {
            let span = window.end - window.start;
            let bl = p_bl(slice(trace, window.start - span, window.end + span));
            (auc_flat(peak, bl), (bl, bl))
        }
        Baseline::VerticalDrop => {
            let bl = v_drop_bl(peak).unwrap_or(0.0);
            (auc_flat(peak, bl), (bl, bl))
        }
        Baseline::ValleyToValley => (
            v2v_auc(peak),
            v2v_bl(peak).map_or((0.0, 0.0), |(a, b)| (a.1, b.1)),
        ),
    };
    let (apex, height, fwhm) = apex_fwhm(peak);
    Peak {
        // An empty f32 sum is -0.0; report it as 0.0. Other values are unchanged.
        area: area + 0.0,
        height,
        apex,
        fwhm,
        baseline,
        scans: peak.len(),
    }
}

// MetaboKit DIA's area: (i0 + i1)·Δrt summed, ×30 (minutes → seconds, /2).
fn engine_auc(t: &[(f32, f32)]) -> f32 {
    t.iter()
        .zip(t.iter().skip(1))
        .map(|((rt0, i0), (rt1, i1))| (i0 + i1) * (rt1 - rt0))
        .sum::<f32>()
        * 30.
}
// calculates the trapezoidal area above a flat baseline
fn auc_flat(rt_i_l: &[(f32, f32)], bl: f32) -> f32 {
    rt_i_l
        .windows(2)
        .map(|rti| (0f32.max(rti[0].1 - bl) + 0f32.max(rti[1].1 - bl)) * (rti[1].0 - rti[0].0))
        .sum::<f32>()
        * 30.
}
// estimates a constant baseline from the lower intensity distribution
fn p_bl(rt_i_l: &[(f32, f32)]) -> f32 {
    if rt_i_l.is_empty() {
        return 0.0;
    }
    let mut i_l: Vec<_> = rt_i_l.iter().map(|x| x.1).collect();
    let i = i_l.len() / 20;
    i_l.select_nth_unstable_by(i, |a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    i_l[i]
}
// finds the lowest intensity for a vertical drop baseline
fn v_drop_bl(rt_i_l: &[(f32, f32)]) -> Option<f32> {
    rt_i_l.iter().map(|x| x.1).reduce(f32::min)
}
// identifies the endpoints for a valley to valley baseline
fn v2v_bl(rt_i_l: &[(f32, f32)]) -> Option<((f32, f32), (f32, f32))> {
    rt_i_l.first().map(|ep0| (*ep0, rt_i_l[rt_i_l.len() - 1]))
}
// calculates the area above a valley to valley baseline
fn v2v_auc(rt_i_l: &[(f32, f32)]) -> f32 {
    v2v_bl(rt_i_l).map_or(0., |(ep0, ep1)| {
        let bl: Vec<f32> = rt_i_l
            .iter()
            .map(|x| ep0.1 + (x.0 - ep0.0) * (ep1.1 - ep0.1) / (ep1.0 - ep0.0))
            .collect();
        rt_i_l
            .windows(2)
            .zip(bl.windows(2))
            .map(|(rti, b)| {
                (0f32.max(rti[0].1 - b[0]) + 0f32.max(rti[1].1 - b[1])) * (rti[1].0 - rti[0].0)
            })
            .sum::<f32>()
            * 30.
    })
}
// apex RT, height and full width at half maximum within the window
fn apex_fwhm(t: &[(f32, f32)]) -> (Option<f32>, f32, Option<f32>) {
    // interpolates the retention time at a target intensity
    fn pred_rt(rti: &[(f32, f32)], hm: f32) -> f32 {
        let rise = rti[1].1 - rti[0].1;
        if rise == 0.0 {
            return rti[0].0;
        }
        rti[0].0 + (hm - rti[0].1) * (rti[1].0 - rti[0].0) / rise
    }
    let Some((max_i, &(rt, height))) = t
        .iter()
        .enumerate()
        .max_by(|x, y| x.1.1.partial_cmp(&y.1.1).unwrap_or(Ordering::Equal))
    else {
        return (None, 0.0, None);
    };
    let hm = height / 2.;
    let fwhm = t[max_i..]
        .windows(2)
        .rfind(|rti01| rti01[1].1 <= hm && hm <= rti01[0].1)
        .map_or_else(
            || t[max_i..].last().map(|x| x.0),
            |rti1| Some(pred_rt(rti1, hm)),
        )
        .and_then(|h1| {
            t[..=max_i]
                .windows(2)
                .find(|rti01| rti01[0].1 <= hm && hm <= rti01[1].1)
                .map_or_else(
                    || t[..=max_i].first().map(|x| x.0),
                    |rti0| Some(pred_rt(rti0, hm)),
                )
                .map(|x| h1 - x)
        });
    (Some(rt), height, fwhm)
}

#[cfg(test)]
mod tests {
    use super::*;
    const W: Window = Window {
        start: 1.0,
        end: 2.0,
    };
    // A triangle on a sloped floor, sampled every 0.25 min.
    fn trace() -> Vec<(f32, f32)> {
        [
            (0.0, 2.),
            (0.25, 2.),
            (0.5, 2.),
            (0.75, 2.),
            (1.0, 4.),
            (1.25, 12.),
            (1.5, 20.),
            (1.75, 12.),
            (2.0, 6.),
            (2.25, 6.),
            (2.5, 6.),
            (2.75, 6.),
        ]
        .into()
    }

    #[test]
    fn none_matches_the_engine_expression_and_excludes_the_end_scan() {
        let p = integrate(&trace(), W, Baseline::None);
        assert_eq!(p.scans, 4);
        // (4+12 + 12+20 + 20+12) * 0.25 * 30
        assert_eq!(p.area, 600.0);
        assert_eq!(p.apex, Some(1.5));
        assert_eq!(p.height, 20.0);
        assert_eq!(p.baseline, (0.0, 0.0));
    }

    #[test]
    fn baselines_follow_mrmhub_definitions() {
        let t = trace();
        let drop = integrate(&t, W, Baseline::VerticalDrop);
        assert_eq!(drop.baseline, (4.0, 4.0));
        assert_eq!(drop.area, (8. + 24. + 24.) * 0.25 * 30.);
        let v2v = integrate(&t, W, Baseline::ValleyToValley);
        assert_eq!(v2v.baseline, (4.0, 12.0));
        // Line from (1, 4) to (1.75, 12): the last scan sits on the baseline.
        assert!(v2v.area > 0.0 && v2v.area < drop.area);
        // 5th percentile of the scans from 0.0 to 2.75 min: the floor.
        let p = integrate(&t, W, Baseline::Percentile);
        assert_eq!(p.baseline, (2.0, 2.0));
        assert_eq!(p.area, (12. + 28. + 28.) * 0.25 * 30.);
    }

    #[test]
    fn fwhm_interpolates_half_height_crossings() {
        let p = integrate(
            &trace(),
            Window {
                start: 1.0,
                end: 2.1,
            },
            Baseline::None,
        );
        // Half height 10: rising 4→12 crosses at 1.1875, falling 12→6 at 1.8333.
        let fwhm = p.fwhm.unwrap();
        assert!((fwhm - (1.8333334 - 1.1875)).abs() < 1e-5, "{fwhm}");
    }

    #[test]
    fn empty_and_flat_windows_are_finite() {
        let p = integrate(
            &trace(),
            Window {
                start: 5.0,
                end: 6.0,
            },
            Baseline::Percentile,
        );
        assert_eq!((p.area, p.scans, p.apex, p.fwhm), (0.0, 0, None, None));
        let flat = [(0.0, 5.0), (0.1, 5.0), (0.2, 5.0)];
        let p = integrate(
            &flat,
            Window {
                start: 0.0,
                end: 1.0,
            },
            Baseline::ValleyToValley,
        );
        assert_eq!(p.area, 0.0);
        assert!(p.fwhm.unwrap().is_finite());
    }
}
