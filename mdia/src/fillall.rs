use rayon::prelude::*;
pub fn fill(param_t: &crate::Param, ana_l: &[f32]) -> std::io::Result<()> {
    let crate::Param { mzml_fs, win_s, .. } = param_t;
    let &crate::Param {
        ms1tol,
        ms2tol,
        impute_width,
        ..
    } = param_t;
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_path("Report.csv")?;
    let headers = rdr.headers().unwrap();
    let idn = headers.iter().position(|x| x == "id_number").unwrap();
    let ms_leveli = headers.iter().position(|x| x == "MS_level").unwrap();
    let featmzi = headers.iter().position(|x| x == "precursor_m/z").unwrap();
    let fragmzi = headers.iter().position(|x| x == "fragment_m/z").unwrap();
    let mrti = headers.iter().position(|x| x == "RT").unwrap();
    let starti = headers.iter().position(|x| x == "RT").unwrap() + 1;
    let pos: csv::Position = rdr.records().reader().position().clone();
    let mz_rt_l: Vec<(usize, u8, f32, f32, f32)> = rdr
        .records()
        .map(std::result::Result::unwrap)
        .map(|x| {
            (
                x[idn].parse().unwrap(),
                x[ms_leveli].parse().unwrap(),
                x[featmzi].parse().unwrap(),
                x[fragmzi].parse().unwrap_or(f32::NAN),
                x[mrti].parse().unwrap(),
            )
        })
        .collect();
    let q_mat: Vec<Vec<f32>> = mzml_fs
        .par_iter()
        .enumerate()
        .map(|(ii, mzml_f)| {
            let mut rdr = csv::ReaderBuilder::new()
                .has_headers(true)
                .trim(csv::Trim::All)
                .from_path("Report.csv")
                .unwrap();
            let mut prev_w = (0f32, 0f32);
            let bn = mzml_f.file_name().unwrap().to_str().unwrap();

            let mut al_rt = Vec::new();
            let ms1_scans = crate::common::get_ms1(bn).unwrap();
            let ms2_scans = crate::common::get_ms2(bn).unwrap();
            rdr.records()
                .zip(&mz_rt_l)
                .map(|(rec, (idn, ms_l, pmz, f_mz, med_rt))| {
                    let mut sel_w = (0f32, 0f32);
                    let mut prev = 0.0;
                    for s_w in win_s.iter().filter(|x| x.0 < *pmz && *pmz < x.1) {
                        let cur = (pmz - s_w.0).min(s_w.1 - pmz);
                        if cur > prev {
                            sel_w = *s_w;
                            prev = cur;
                        }
                    }
                    if sel_w != prev_w {
                        al_rt = crate::score::get_swath(&ms2_scans, sel_w);
                        prev_w = sel_w;
                    }

                    let impute_width = if impute_width > 0. {
                        impute_width
                    } else {
                        ana_l[idn - 1]
                    };
                    rec.as_ref().unwrap()[starti + ii]
                        .parse::<f32>()
                        .unwrap_or_else(|_| {
                            let p_maxi = if *ms_l == 1 {
                                crate::common::get_chrom(
                                    (*pmz, *med_rt, impute_width),
                                    &ms1_scans,
                                    ms1tol,
                                )
                            } else {
                                crate::common::get_chrom2(
                                    (*f_mz, *med_rt, impute_width),
                                    &al_rt,
                                    ms2tol,
                                )
                            };
                            p_maxi
                                .iter()
                                .zip(p_maxi.iter().skip(1))
                                .map(|((rt0, i0), (rt1, i1))| (i0 + i1) * (rt1 - rt0))
                                .sum::<f32>()
                                * 30.
                        })
                })
                .collect()
        })
        .collect();
    let mut wtr = csv::WriterBuilder::new()
        .from_path("Report_fill_all.csv")
        .unwrap();
    wtr.write_record(rdr.headers().unwrap())?;
    rdr.records().reader_mut().seek(pos)?;
    let mut qrow = Vec::with_capacity(q_mat.len());
    for (jj, rec) in rdr.records().enumerate() {
        qrow.clear();
        qrow.extend(q_mat.iter().map(|x| format!("{:.1}", x[jj])));
        let line: &csv::StringRecord = rec.as_ref().unwrap();
        wtr.write_record(
            line.iter()
                .take(starti)
                .chain(qrow.iter().map(std::string::String::as_str))
                .chain(line.iter().skip(starti + mzml_fs.len())),
        )?;
    }
    Ok(())
}
