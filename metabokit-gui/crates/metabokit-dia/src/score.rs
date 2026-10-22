// Adapted from MetaboKit DIA (SLING, BSD-3-Clause); see LICENSE.
use std::fs::File;
use std::io::BufReader;
struct Peak {
    mz: f32,
    rt: f32,
    sc: f32,
    #[allow(dead_code)] // Retained in the original feature cache layout.
    coef: f32,
    shape: f32,
    smooth: f32,
}

fn ms1feat(bn: &str, param_t: &crate::Param) -> Result<Vec<Peak>, crate::Failure> {
    use crate::common::{unpack_f32, unpack_u32};
    let file_path = param_t.misc().join(format!("ms1f_{bn}.bin"));
    let buf = &mut BufReader::new(File::open(file_path)?);
    let len0 = unpack_u32(buf)?;
    (0..len0)
        .map(|_| {
            Ok(Peak {
                mz: unpack_f32(buf)?,
                rt: unpack_f32(buf)?,
                sc: unpack_f32(buf)?,
                coef: unpack_f32(buf)?,
                shape: unpack_f32(buf)?,
                smooth: unpack_f32(buf)?,
            })
        })
        .collect()
}
#[must_use]
pub fn get_swath(ms2_scans: &[crate::Msms], lo_hi: (f32, f32)) -> Vec<(f32, &Vec<(f32, f32)>)> {
    let swath: Vec<_> = ms2_scans
        .iter()
        .filter(|x| lo_hi.0 < x.ms1mz && x.ms1mz < lo_hi.1)
        .map(|x| (x.rt, &x.mz_i_l))
        .collect();
    swath
}
fn cos_sim(l1l2: &[(f32, f32)]) -> f32 {
    let l1sum = l1l2.iter().map(|x| x.0).sum::<f32>();
    let l2sum = l1l2.iter().map(|x| x.1).sum::<f32>();
    if l1sum <= 0. || l2sum <= 0. {
        return 0.;
    }
    let numer = l1l2.iter().map(|x| (x.0 * x.1).sqrt()).sum::<f32>();
    let denom = (l1sum * l2sum).sqrt();
    numer / denom
}
fn cor0(l1: &[f32], l2: &[f32]) -> f32 {
    if l1.iter().sum::<f32>() <= 0.0 || l2.iter().sum::<f32>() <= 0.0 {
        return 0.0;
    }
    let m1 = l1.iter().sum::<f32>() / l1.len() as f32;
    let m2 = l2.iter().sum::<f32>() / l2.len() as f32;
    let numer = l1
        .iter()
        .zip(l2)
        .map(|x| (x.0 - m1) * (x.1 - m2))
        .sum::<f32>();
    let denom = l1.iter().map(|x| (x - m1).powi(2)).sum::<f32>()
        * l2.iter().map(|x| (x - m2).powi(2)).sum::<f32>();
    numer / denom.sqrt()
}

pub fn score(
    param_t: &crate::Param,
    bn: &str,
    lib_ent: &[super::readlib::Ent],
    ms1_scans: &[crate::Ms],
    ms2_scans: &[crate::Msms],
) -> Result<Vec<super::Cpd>, crate::Failure> {
    let &crate::Param {
        ms1tol,
        ms2tol,
        ms2_score,
        min_peaks,
        rt_shift,
        i_rt,
        i_mz,
        ..
    } = param_t;
    let ms1peaks = ms1feat(bn, param_t)?;
    let mut prev_w = (0f32, 0f32);
    let mut al_rt = Vec::new();
    let mut feat_f_sco = Vec::new();
    let mut ent_ms2_i = Vec::<(f32, f32)>::new();
    for (ent_i, ent) in lib_ent.iter().enumerate() {
        param_t.check()?;
        let mut sel_w = (0f32, 0f32);
        let mut prev = 0.0;
        for s_w in param_t
            .win_s
            .iter()
            .filter(|x| x.0 < ent.mmass && ent.mmass < x.1)
        {
            let cur = (ent.mmass - s_w.0).min(s_w.1 - ent.mmass);
            if cur > prev {
                sel_w = *s_w;
                prev = cur;
            }
        }
        if sel_w != prev_w {
            al_rt = get_swath(ms2_scans, sel_w);
            prev_w = sel_w;
        }
        if al_rt.is_empty() {
            continue;
        }
        let sel_ms2 = &ent.mz_i;

        let pos0 = ent.mmass - ms1tol;
        let pos0 = ms1peaks.partition_point(|x| x.mz < pos0);
        let ent_mmass_tol = ent.mmass + ms1tol;
        let mut cs_l = Vec::<(f32, &Peak)>::new();
        for ms1f in ms1peaks[pos0..]
            .iter()
            .take_while(|x| x.mz < ent_mmass_tol)
            .filter(|x| ent.rt.is_some_and(|rt| (x.rt - rt).abs() < rt_shift))
        {
            let (_, mz_i_l) = al_rt
                .iter()
                .map(|x| ((x.0 - ms1f.rt).abs(), x.1))
                .min_by(|x, y| x.0.partial_cmp(&y.0).unwrap())
                .unwrap();
            ent_ms2_i.clear();
            for (f_mz, f_i) in sel_ms2 {
                let pos0 = f_mz - ms2tol;
                let pos0 = mz_i_l.partition_point(|x| x.0 < pos0);
                let f_mz_tol = f_mz + ms2tol;
                ent_ms2_i.push((
                    *f_i,
                    mz_i_l[pos0..]
                        .iter()
                        .take_while(|x| x.0 < f_mz_tol)
                        .map(|x| x.1)
                        .reduce(f32::max)
                        .unwrap_or(0.0),
                ));
            }
            if usize::from(min_peaks) <= ent_ms2_i.iter().filter(|x| x.1 > 0.).count() {
                let cs = cos_sim(&ent_ms2_i);
                cs_l.push((cs, ms1f));
            }
        }
        let Some(((max_cs, ms1f), _)) = cs_l
            .iter()
            .filter(|x| ms2_score < x.0)
            .filter_map(|x| ent.rt.map(|y| (x, (y - x.1.rt).abs())))
            .min_by(|x, y| x.1.partial_cmp(&y.1).unwrap())
        else {
            continue;
        };
        let chrom1 = crate::common::get_chrom((ms1f.mz, ms1f.rt, ms1f.sc * i_rt), ms1_scans, i_mz);
        let ms1_auc = chrom1
            .iter()
            .zip(&chrom1[1..])
            .map(|((rt0, i0), (rt1, i1))| (i0 + i1) * (rt1 - rt0))
            .sum::<f32>()
            * 30.;
        let pos0 = ms1f.rt - ms1f.sc;
        let pos0 = chrom1.partition_point(|x| x.0 < pos0);
        let pos1 = ms1f.rt + ms1f.sc;
        let pos1 = pos0 + chrom1[pos0..].partition_point(|x| x.0 < pos1);
        let s_n = chrom1[pos0..pos1]
            .iter()
            .map(|(.., i)| *i)
            .reduce(f32::max)
            .unwrap()
            / (chrom1[..pos0]
                .iter()
                .chain(&chrom1[pos1..])
                .map(|(.., i)| *i)
                .sum::<f32>()
                / (chrom1.len() - pos1 + pos0).max(1) as f32)
                .max(1.);
        let pre = super::MzAucC {
            mz: ms1f.mz,
            auc: ms1_auc,
            score: *max_cs,
            s_n,
        };
        let mut mz_auc_c = Vec::new();
        for (f_mz, _) in sel_ms2 {
            let chrom2 =
                crate::common::get_chrom2((*f_mz, ms1f.rt, ms1f.sc * i_rt), &al_rt, ms2tol);
            let f_auc = chrom2
                .iter()
                .zip(chrom2.iter().skip(1))
                .map(|((rt0, i0), (rt1, i1))| (i0 + i1) * (rt1 - rt0))
                .sum::<f32>()
                * 30.;
            let f_cor = cor0(
                &chrom1.iter().map(|x| x.1).collect::<Vec<_>>(),
                &chrom1
                    .iter()
                    .map(|x| {
                        chrom2
                            .iter()
                            .map(|(rt, i)| ((x.0 - rt).abs(), i))
                            .min_by(|x, y| x.0.partial_cmp(&y.0).unwrap())
                            .unwrap()
                    })
                    .map(|x| *x.1)
                    .collect::<Vec<_>>(),
            );
            let pos0 = ms1f.rt - ms1f.sc;
            let pos0 = chrom2.partition_point(|x| x.0 < pos0);
            let pos1 = ms1f.rt + ms1f.sc;
            let pos1 = pos0 + chrom2[pos0..].partition_point(|x| x.0 < pos1);
            let s_n = chrom2[pos0..pos1]
                .iter()
                .map(|(.., i)| *i)
                .reduce(f32::max)
                .unwrap_or(0.)
                / (chrom2[..pos0]
                    .iter()
                    .chain(&chrom2[pos1..])
                    .map(|(.., i)| *i)
                    .sum::<f32>()
                    / (chrom2.len() - pos1 + pos0).max(1) as f32)
                    .max(1.);
            mz_auc_c.push(super::MzAucC {
                mz: *f_mz,
                auc: f_auc,
                score: f_cor,
                s_n,
            });
        }
        feat_f_sco.push(super::Cpd {
            i: 0,
            ent_i,
            rt: ms1f.rt,
            sc: ms1f.sc,
            shape: ms1f.shape,
            smooth: ms1f.smooth,
            pre,
            mz_auc_c,
        });
    }
    Ok(feat_f_sco)
}
