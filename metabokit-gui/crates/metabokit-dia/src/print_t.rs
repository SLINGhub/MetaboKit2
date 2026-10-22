// Adapted from MetaboKit DIA (SLING, BSD-3-Clause); see LICENSE.
use crate::common::{unpack_f32, unpack_f32_2, unpack_string, unpack_u8, unpack_u16};
use std::fs::File;

#[derive(Clone, Copy)]
struct RtScCA {
    rt: f32,
    auc: f32,
}
struct Co {
    mz: f32,
    rt: f32,
}

struct Analyte {
    name: String,
    mmass: f32,
    f_mz_l: Vec<(f32, f32)>,
    mz_rt_sc: Vec<Co>,
}

use crate::Param;
pub fn print_tar(param_t: &Param) -> Result<(), crate::Failure> {
    let Param { mzml_fs, .. } = param_t;
    let &Param { i_mz, .. } = param_t;
    let targets0 = tar_list(mzml_fs.len(), param_t)?;

    let mut wtr =
        csv::WriterBuilder::new().from_path(param_t.output_dir.join("target_list.csv"))?;
    wtr.write_record([
        "name",
        "Precursor_Ion",
        "Product_Ion",
        "RT",
        "uniform_width (y/n)",
        "start_RT",
        "end_RT",
        "peak_width",
        "RT_shift",
        "baseline",
        "remarks",
    ])?;
    let wave_scs = [0.05, 0.1];
    let wave_sqrt = wave_scs.map(f32::sqrt);
    let m_sc = 0.3;
    // Keep only compact ridge results across samples; never all MS1 scans.
    let mut features: Vec<Vec<Vec<RtScCA>>> = targets0
        .iter()
        .map(|_| Vec::with_capacity(mzml_fs.len()))
        .collect();
    for (sample, path) in mzml_fs.iter().enumerate() {
        param_t.check()?;
        let bn = path.file_name().unwrap().to_str().unwrap();
        let ms1_scans = crate::common::get_ms1(&param_t.misc(), bn)?;
        for (target, trans_feats) in targets0.iter().zip(&mut features) {
            param_t.check()?;
            let tar0 = &target.mz_rt_sc[sample];
            let rt_i_l = crate::common::get_chrom((tar0.mz, tar0.rt, m_sc), &ms1_scans, i_mz);
            if rt_i_l.len() < 3 {
                return Err(
                    format!("{bn}: insufficient MS1 scans for target {}", target.name).into(),
                );
            }
            let first_dec = {
                let mut i_l: Vec<_> = rt_i_l.iter().map(|x| x.1).collect();
                let i = i_l.len() / 20;
                i_l.select_nth_unstable_by(i, |a, b| a.partial_cmp(b).unwrap());
                i_l[i] * 1.1
            };
            trans_feats.push(findridge(&rt_i_l, first_dec, &wave_scs, &wave_sqrt, 0.));
        }
    }
    for (target0, trans_feats) in targets0.iter().zip(features) {
        let median_rt = calc_med_rt(&trans_feats);
        let mmass = format!("{:.4}", target0.mmass);
        for rt in median_rt.iter().map(|rt| format!("{rt:.2}")) {
            wtr.write_field(&target0.name)?;
            wtr.write_field(&mmass)?;
            wtr.write_field("")?;
            wtr.write_field(&rt)?;
            wtr.write_field("y")?;
            for _ in 0..5 {
                wtr.write_field("")?;
            }
            wtr.write_field(if median_rt.len() == 1 { "" } else { &rt })?;
            wtr.write_record(None::<&[u8]>)?;
            for (mz, _) in &target0.f_mz_l {
                wtr.write_field(&target0.name)?;
                wtr.write_field(&mmass)?;
                wtr.write_field(format!("{mz:.3}"))?;
                wtr.write_field(&rt)?;
                wtr.write_field("y")?;
                for _ in 0..5 {
                    wtr.write_field("")?;
                }
                wtr.write_field(if median_rt.len() == 1 { "" } else { &rt })?;
                wtr.write_record(None::<&[u8]>)?;
            }
        }
    }
    Ok(())
}

fn tar_list(nfiles: usize, param_t: &crate::Param) -> Result<Vec<Analyte>, crate::Failure> {
    let file_path = param_t.misc().join("analyte.bin");
    let buf = &mut std::io::BufReader::new(File::open(file_path)?);
    let mut ana_l = Vec::new();
    while let Ok(name) = unpack_string(buf) {
        let mmass = unpack_f32(buf)?;
        let len = i64::from(unpack_u16(buf)?);
        let f_mz_l = (0..len)
            .map(|_| unpack_f32_2(buf))
            .collect::<Result<_, _>>()?;
        let mz_rt_sc = (0..nfiles)
            .map(|_| {
                let pres = unpack_u8(buf)?;
                let rt = unpack_f32(buf)?;
                buf.seek_relative(4)?;
                let mz = unpack_f32(buf)?;
                buf.seek_relative(i64::from(pres) * (12 + 8 * len))?;
                Ok(Co { mz, rt })
            })
            .collect::<Result<_, crate::Failure>>()?;
        ana_l.push(Analyte {
            name,
            mmass,
            f_mz_l,
            mz_rt_sc,
        });
    }
    Ok(ana_l)
}
fn findridge(
    rt_i_l: &[(f32, f32)],
    first_dec: f32,
    wave_scs: &[f32],
    wave_sqrt: &[f32],
    sh: f32,
) -> Vec<RtScCA> {
    struct RtScC {
        rt: f32,
        sc: f32,
        coef: f32,
    }
    let mut rt_i_l: Vec<(f32, f32)> = rt_i_l.to_vec();
    for j in &mut rt_i_l {
        j.1 = 0f32.max(j.1 - first_dec);
    }
    let rerunw = 2. * (rt_i_l.last().unwrap().0 - rt_i_l[0].0) / (rt_i_l.len() - 1) as f32;
    let i_cut = {
        let mut i_l = rt_i_l.iter().map(|x| x.1).collect::<Vec<f32>>();
        let i = (0.1 / (rt_i_l.last().unwrap().0 - rt_i_l[0].0) * rt_i_l.len() as f32) as usize;
        i_l.select_nth_unstable_by(i, |b, a| a.partial_cmp(b).unwrap());
        i_l[i]
    };
    let mut prev = true;
    let (eic_rt, mut calc_p): (Vec<_>, Vec<_>) = rt_i_l
        .iter()
        .zip(&rt_i_l[1..])
        .filter_map(|((rt0, i0), (rt1, i1))| {
            let c = *i0 > i_cut || *i1 > i_cut;
            (c || prev).then(|| {
                prev = c;
                (rt0.midpoint(*rt1), c)
            })
        })
        .unzip();
    calc_p[0] = false;
    calc_p[eic_rt.len() - 1] = false;
    let bisect = |a| rt_i_l.partition_point(|x| x.0 < a);
    let mut coefs = vec![0f32; eic_rt.len() * wave_scs.len()];
    let mut int_i = Vec::new();
    for ((wave_s, w_sqrt), coef_xx) in wave_scs
        .iter()
        .zip(wave_sqrt)
        .zip(coefs.chunks_exact_mut(eic_rt.len()))
    {
        let wave_s_ = wave_s.recip();
        for ((yy, wave_loc), _) in coef_xx
            .iter_mut()
            .zip(&eic_rt)
            .zip(&calc_p)
            .filter(|x| *x.1)
        {
            let pos0 = bisect(wave_loc - rerunw);
            let pos1 = bisect(wave_loc + rerunw);
            let rt_i = &rt_i_l[pos0..pos1];
            if rt_i.is_empty() {
                println!(
                    "missing MS1 scans between {:.2} and {:.2} minutes",
                    rt_i_l[pos0 - 1].0,
                    rt_i_l[pos0].0
                );
                continue;
            }
            int_i.clear();
            int_i.extend(rt_i.iter().map(|(rt0, i0)| {
                let tsig2 = ((rt0 - wave_loc) * wave_s_).powi(2);
                i0 * (-tsig2 / 2.).exp() * (1. - tsig2)
            }));
            *yy = rt_i
                .iter()
                .zip(&int_i)
                .zip(&rt_i[1..])
                .zip(&int_i[1..])
                .map(|((((rt0, _), i0), (rt1, _)), i1)| (i0 + i1) * (rt1 - rt0))
                .sum::<f32>()
                / w_sqrt;
        }
    }
    let mut local_max = Vec::new();
    for coef_xx in coefs.chunks_exact_mut(eic_rt.len()) {
        let mut l_max = Vec::new();
        loop {
            let (max_i, max_coef) = coef_xx
                .iter()
                .copied()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                .unwrap();
            if max_coef <= 0. {
                break;
            }
            if coef_xx[max_i - 1] <= 0. || coef_xx[max_i + 1] <= 0. {
                coef_xx[max_i] = 0.;
                continue;
            }
            l_max.push((max_i, max_coef));
            let lo = eic_rt[max_i] - rerunw;
            let up = eic_rt[max_i] + rerunw;
            let lo = eic_rt[..max_i].partition_point(|x| *x <= lo);
            let up = max_i + eic_rt[max_i..].partition_point(|x| *x < up);
            coef_xx[lo..up].fill(0.);
        }
        local_max.push(l_max);
    }
    let mut ridgels: Vec<Vec<RtScC>> = local_max[0]
        .iter()
        .map(|&(max_i, coef)| {
            vec![RtScC {
                rt: eic_rt[max_i],
                sc: wave_scs[0],
                coef,
            }]
        })
        .collect();
    for (xx, scale_coef) in local_max[1..].iter().enumerate() {
        for &(max_i, coef) in scale_coef {
            let rt = eic_rt[max_i];
            if let Some(rl) = ridgels.iter_mut().find(|rl| {
                let rl_last = rl.last().unwrap();
                rl_last.sc == wave_scs[xx]
                    && ((rl_last.rt - rt).abs() < 0.01
                        || bisect(rt).abs_diff(bisect(rl_last.rt)) < 2)
            }) {
                rl.push(RtScC {
                    rt,
                    sc: wave_scs[xx + 1],
                    coef,
                });
            } else {
                ridgels.push(vec![RtScC {
                    rt,
                    sc: wave_scs[xx + 1],
                    coef,
                }]);
            }
        }
    }
    let peaks: Vec<RtScCA> = ridgels
        .into_iter()
        .filter(|x| x.len() > 1)
        .map(|rd| {
            let p_loc = rd
                .into_iter()
                .max_by(|a, b| a.coef.partial_cmp(&b.coef).unwrap())
                .unwrap();
            let pos = bisect(p_loc.rt);
            RtScCA {
                rt: p_loc.rt - sh,
                auc: rt_i_l[pos - 1].1 + rt_i_l[pos].1,
            }
        })
        .collect();
    if peaks.is_empty() {
        return vec![RtScCA {
            rt: rt_i_l[0].0.midpoint(rt_i_l.last().unwrap().0) - sh,
            auc: 1.,
        }];
    }
    let largest_coef = peaks
        .iter()
        .max_by(|x, y| x.auc.partial_cmp(&y.auc).unwrap())
        .unwrap()
        .auc
        * 0.1;
    peaks.into_iter().filter(|x| largest_coef < x.auc).collect()
}
fn calc_med_rt(trans_feats: &[Vec<RtScCA>]) -> Vec<f32> {
    let med_npeaks = {
        let mut npeaks_l: Vec<_> = trans_feats.iter().map(std::vec::Vec::len).collect();
        let i = npeaks_l.len() / 2;
        npeaks_l.select_nth_unstable(i);
        npeaks_l[i].min(12)
    };
    let mut median_rt = Vec::new();
    let mut feats = Vec::<RtScCA>::new();
    let mut feats_rt = Vec::<f32>::new();
    let mut rt_l = Vec::<f32>::new();
    for npeaks in (1..=med_npeaks).rev() {
        if npeaks < median_rt.len() {
            break;
        }
        feats_rt.clear();
        for feats_ in trans_feats.iter().filter(|x| npeaks <= x.len()) {
            feats.extend(feats_);
            if npeaks < feats.len() {
                feats.select_nth_unstable_by(npeaks - 1, |y, x| x.auc.partial_cmp(&y.auc).unwrap());
                feats.truncate(npeaks);
            }
            feats.sort_unstable_by(|x, y| x.rt.partial_cmp(&y.rt).unwrap());
            feats_rt.extend(feats.drain(..).map(|x| x.rt));
        }
        let mt = trans_feats.len() / 2 + 1;
        rt_l.resize(feats_rt.len() / npeaks, 0.);
        let mf = rt_l.len() / 2;
        let cm: Vec<f32> = (0..npeaks)
            .map(|i| {
                rt_l.iter_mut()
                    .zip(feats_rt[i..].iter().step_by(npeaks))
                    .for_each(|(dest, src)| *dest = *src);
                rt_l.select_nth_unstable_by(mf, |x, y| x.partial_cmp(y).unwrap());
                rt_l[mf]
            })
            .filter(|x| {
                trans_feats
                    .iter()
                    .filter(|f| f.iter().any(|z| (z.rt - x).abs() < 0.025))
                    .count()
                    >= mt
            })
            .collect();
        if median_rt.len() <= cm.len() {
            median_rt = cm;
        }
    }
    if median_rt.is_empty() {
        return vec![rt_l[rt_l.len() / 2]];
    }
    median_rt
}
