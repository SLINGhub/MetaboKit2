use crate::common::median;
use std::fs::File;
use std::io::Write;

#[derive(Clone)]
struct AucRT<'a> {
    rt: f32,
    sc: f32,
    shape: f32,
    smooth: f32,
    pre: &'a super::MzAucC,
    mz_auc_c: Vec<&'a super::MzAucC>,
}
struct Iddat {
    inchikey: String,
    acc_hmdb: String,
    name_hmdb: String,
    super_hmdb: String,
    class_hmdb: String,
    sub_hmdb: String,
    lm_id: String,
    name_lm: String,
    abb_lm: String,
    core_lm: String,
    main_lm: String,
    sub_lm: String,
    abb_c_lm: String,
    formu: String,
}

fn dedup0<'a, F, G>(x: &[&'a G], f: F) -> Vec<&'a str>
where
    F: Fn(&&'a G) -> &'a str,
{
    let mut xx: Vec<&str> = x.iter().map(f).filter(|x| !x.is_empty()).collect();
    xx.sort_unstable();
    xx.dedup();
    xx
}

pub fn print_tab(
    param_t: &crate::Param,
    lib_ent: &[super::readlib::Ent],
    all_feat: Vec<Vec<super::Cpd>>,
) -> std::io::Result<Vec<f32>> {
    let name_formu = read_name_f();
    let iddat_d = read_inchik();
    let iddat_i14: Vec<&str> = iddat_d
        .iter()
        .map(|x| &x.inchikey[..14.min(x.inchikey.len())])
        .collect();

    let crate::Param { mzml_fs, .. } = param_t;
    let &crate::Param {
        rt_shift,
        i_rt,
        impute_width,
        ..
    } = param_t;
    let mut all_feat: Vec<_> = all_feat
        .into_iter()
        .enumerate()
        .flat_map(|(i, mut feats)| {
            for f in &mut feats {
                f.i = i;
            }
            feats
        })
        .collect();
    all_feat.sort_unstable_by_key(|x| x.ent_i);

    let mut clust_grp = Vec::<Vec<usize>>::new();
    let mut inc_l: Vec<bool> = (0..lib_ent.len())
        .map(|x| all_feat.binary_search_by_key(&x, |y| y.ent_i).is_ok())
        .collect();
    for (pos0, ent) in lib_ent.iter().enumerate() {
        if inc_l[pos0] {
            let ent_mmass_01 = ent.mmass + 0.001;
            let mut rt_vec: Vec<_> = lib_ent[pos0..]
                .iter()
                .take_while(|x| x.mmass < ent_mmass_01)
                .filter_map(|x| x.rt)
                .collect();
            rt_vec.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
            let mut breakpt = vec![rt_vec[0] - 1.0];
            breakpt.extend(
                rt_vec
                    .iter()
                    .zip(rt_vec.iter().skip(1))
                    .filter(|x| x.1 - x.0 > rt_shift)
                    .map(|x| x.0.midpoint(*x.1))
                    .chain(std::iter::once(rt_vec.last().unwrap() + 1.)),
            );
            let (rt0, rt1) = breakpt
                .iter()
                .zip(&breakpt[1..])
                .find(|x| ent.rt.is_some_and(|rt_| *x.0 < rt_ && rt_ < *x.1))
                .unwrap();
            let mut clust = vec![pos0];
            for (pos, _) in (pos0 + 1..)
                .zip(&lib_ent[pos0 + 1..])
                .take_while(|(_, x)| x.mmass < ent_mmass_01)
                .filter(|(_, x)| x.rt.is_some_and(|x| *rt0 < x && x < *rt1))
            {
                if inc_l[pos] {
                    inc_l[pos] = false;
                    clust.push(pos);
                }
            }
            clust_grp.push(clust);
        }
    }
    let mut wtr = csv::WriterBuilder::new().from_path("Report.csv")?;
    let mut line: Vec<String> = Vec::new();
    for h in [
        "group_number",
        "id_number",
        "name",
        "super class (HMDB)",
        "class (HMDB)",
        "sub class (HMDB)",
        "accession (HMDB)",
        "name (HMDB)",
        "core (LMSD)",
        "main_class (LMSD)",
        "sub_class (LMSD)",
        "lm_id (LMSD)",
        "name (LMSD)",
        "abbrev (LMSD)",
        "abbrev_chains (LMSD)",
        "InChIKey",
        "formula",
        "MS_level",
        "adduct",
        "precursor_m/z",
        "fragment_m/z",
        "RT (library)",
        "RT",
    ] {
        wtr.write_field(h)?;
    }
    let bns: Vec<&str> = mzml_fs
        .iter()
        .map(|x| x.file_stem().unwrap().to_str().unwrap())
        .collect();
    wtr.write_record(
        ["AREA", "RT", "SCORE", "S/N"]
            .into_iter()
            .flat_map(|pref| bns.iter().map(move |bn| [pref, "_", bn].concat())),
    )?;

    write_file_ord(mzml_fs)?;
    let file_path = std::path::Path::new(crate::MISCDIR).join("analyte.bin");
    let mut buf = std::io::BufWriter::new(File::create(file_path)?);

    let mut p_auc_l = vec![None::<AucRT>; mzml_fs.len()];
    let mut ana_l = Vec::new();
    for (ci, clust) in (1u32..).zip(
        (1..)
            .zip(clust_grp)
            .flat_map(|(grp, clust)| clust.into_iter().map(move |x| (grp, x))),
    ) {
        let mut cpd_row: Vec<_> = all_feat[all_feat.partition_point(|x| x.ent_i < clust.1)..]
            .iter()
            .take_while(|x| x.ent_i == clust.1)
            .collect();
        p_auc_l.fill(None);
        cpd_row.sort_unstable_by_key(|x| x.i);
        for max_cs in cpd_row.chunk_by(|a, b| a.i == b.i).map(|ch| {
            ch.iter()
                .max_by(|x, y| x.pre.score.partial_cmp(&y.pre.score).unwrap())
                .unwrap()
        }) {
            p_auc_l[max_cs.i] = Some(AucRT {
                rt: max_cs.rt,
                sc: max_cs.sc,
                shape: max_cs.shape,
                smooth: max_cs.smooth,
                pre: &max_cs.pre,
                mz_auc_c: {
                    let mut mz_auc_c: Vec<_> = max_cs.mz_auc_c.iter().collect();
                    mz_auc_c.sort_unstable_by(|a, b| a.mz.partial_cmp(&b.mz).unwrap());
                    mz_auc_c
                },
            });
        }
        let ent_ = &lib_ent[clust.1];

        let mut iddats: Vec<&Iddat> = {
            let x = ent_.inchik.as_str();
            iddat_d[iddat_d.partition_point(|y| y.inchikey.as_str() < x)..]
                .iter()
                .take_while(|y| y.inchikey.as_str() == x)
                .collect()
        };
        if iddats.is_empty() {
            iddats = {
                let x = &ent_.inchik;
                let x = &x[..14.min(x.len())];
                let pos0 = iddat_i14.partition_point(|y| *y < x);
                iddat_d[pos0..]
                    .iter()
                    .zip(iddat_i14[pos0..].iter().take_while(move |i14| **i14 == x))
                    .map(|y| y.0)
                    .collect()
            }
        }

        line.clear();
        line.push(clust.0.to_string());
        line.push(ci.to_string());
        line.push(ent_.name.clone());
        buf.write_all(ent_.name.as_bytes())?;
        buf.write_all(b"\0")?;
        buf.write_all(&ent_.mmass.to_le_bytes())?;
        line.push(dedup0(&iddats, |x| &x.super_hmdb).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.class_hmdb).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.sub_hmdb).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.acc_hmdb).join(", "));
        line.push(dedup0(&iddats, |x| &x.name_hmdb).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.core_lm).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.main_lm).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.sub_lm).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.lm_id).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.name_lm).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.abb_lm).join(" --- "));
        line.push(dedup0(&iddats, |x| &x.abb_c_lm).join(" --- "));
        line.push(ent_.inchik.clone());
        line.push(
            if iddats.is_empty() {
                let mut f_ = {
                    let name_f = &ent_.name;
                    let name_f_: &str = if name_f.ends_with(".txt)") {
                        &name_f[..name_f.rfind(" (").unwrap()]
                    } else {
                        name_f
                    };
                    name_formu[name_formu.partition_point(|y| y.0.as_str() < name_f_)..]
                        .iter()
                        .take_while(move |y| [name_f_, name_f].contains(&y.0.as_str()))
                        .map(|x| x.1.as_str())
                        .collect::<Vec<_>>()
                };
                f_.sort_unstable();
                f_.dedup();
                f_
            } else {
                dedup0(&iddats, |x| &x.formu)
            }
            .join(", "),
        );
        line.push("1".to_owned());
        line.push(ent_.adduct.clone());
        line.push(format!("{:.4}", ent_.mmass));
        let m_sc = if impute_width > 0. {
            impute_width
        } else {
            median(
                p_auc_l
                    .iter()
                    .filter_map(|x| x.as_ref().map(|x| x.sc))
                    .collect(),
            ) * i_rt
        };
        let m_rt = median(
            p_auc_l
                .iter()
                .filter_map(|x| x.as_ref().map(|x| x.rt))
                .collect(),
        );
        ana_l.push(m_sc);
        line.push(String::new());
        line.push(ent_.rt.map_or(String::new(), |x| format!("{x:.2}")));
        line.push(format!("{m_rt:.2}"));

        line.extend(p_auc_l.iter().map(|x| {
            x.as_ref()
                .map_or(String::new(), |x| format!("{:.1}", x.pre.auc))
        }));
        line.extend(
            p_auc_l
                .iter()
                .map(|x| x.as_ref().map_or(String::new(), |x| format!("{:.2}", x.rt))),
        );
        line.extend(p_auc_l.iter().map(|x| {
            x.as_ref()
                .map_or(String::new(), |x| format!("{:.2}", x.pre.score))
        }));
        line.extend(p_auc_l.iter().map(|x| {
            x.as_ref()
                .map_or(String::new(), |x| format!("{:.1}", x.pre.s_n))
        }));
        wtr.write_record(&line)?;

        buf.write_all(&(ent_.mz_i.len() as u16).to_le_bytes())?;
        let f_mz_sets: Vec<(f32, f32)> = {
            let mut mz_i_l = ent_.mz_i.clone();
            mz_i_l.sort_unstable_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            mz_i_l
        };
        "2".clone_into(&mut line[17]);
        for (ff, f_mz) in f_mz_sets.iter().enumerate() {
            buf.write_all(&f_mz.0.to_le_bytes())?;
            buf.write_all(&f_mz.1.to_le_bytes())?;
            line.truncate(23);
            line[20] = format!("{:.4}", f_mz.0);


            line.extend(p_auc_l.iter().map(|x| {
                x.as_ref()
                    .map_or(String::new(), |x| format!("{:.1}", x.mz_auc_c[ff].auc))
            }));
            line.extend(
                p_auc_l
                    .iter()
                    .map(|x| x.as_ref().map_or(String::new(), |x| format!("{:.2}", x.rt))),
            );
            line.extend(p_auc_l.iter().map(|x| {
                x.as_ref()
                    .map_or(String::new(), |x| format!("{:.2}", x.mz_auc_c[ff].score))
            }));
            line.extend(p_auc_l.iter().map(|x| {
                x.as_ref()
                    .map_or(String::new(), |x| format!("{:.1}", x.mz_auc_c[ff].s_n))
            }));
            wtr.write_record(&line)?;
        }
        for x in &p_auc_l {
            if let Some(x) = x {
                buf.write_all(&1u8.to_le_bytes())?;
                buf.write_all(&x.rt.to_le_bytes())?;
                buf.write_all(&(x.sc * i_rt).to_le_bytes())?;
                buf.write_all(&x.pre.mz.to_le_bytes())?;
                buf.write_all(&x.pre.auc.to_le_bytes())?;
                buf.write_all(&x.shape.to_le_bytes())?;
                buf.write_all(&x.smooth.to_le_bytes())?;
                for f in &x.mz_auc_c {
                    buf.write_all(&f.score.to_le_bytes())?;
                    buf.write_all(&f.auc.to_le_bytes())?;
                }
            } else {
                buf.write_all(&0u8.to_le_bytes())?;
                buf.write_all(&m_rt.to_le_bytes())?;
                buf.write_all(&m_sc.to_le_bytes())?;
                buf.write_all(&ent_.mmass.to_le_bytes())?;
            }
        }
    }
    Ok(ana_l)
}
fn write_file_ord(mzml_fs: &[std::path::PathBuf]) -> std::io::Result<()> {
    let mut wtr = csv::WriterBuilder::new()
        .delimiter(b'\t')
        .from_path(std::path::Path::new(crate::MISCDIR).join("file_order.txt"))?;
    for bn in mzml_fs
        .iter()
        .map(|x| x.file_name().unwrap().to_str().unwrap())
    {
        wtr.write_record([bn])?;
    }
    Ok(())
}
fn read_inchik() -> Vec<Iddat> {
    let file_path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("libs")
        .join("inchik.txt");
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_path(file_path)
        .unwrap();
    let mut iddat_d: Vec<Iddat> = rdr
        .records()
        .map(std::result::Result::unwrap)
        .map(|x| Iddat {
            inchikey: x[0].to_owned(),
            acc_hmdb: x[1].to_owned(),
            name_hmdb: x[2].to_owned(),
            super_hmdb: x[3].to_owned(),
            class_hmdb: x[4].to_owned(),
            sub_hmdb: x[5].to_owned(),
            lm_id: x[6].to_owned(),
            name_lm: x[7].to_owned(),
            abb_lm: x[8].to_owned(),
            core_lm: x[9].to_owned(),
            main_lm: x[10].to_owned(),
            sub_lm: x[11].to_owned(),
            abb_c_lm: x[12].to_owned(),
            formu: x[13].to_owned(),
        })
        .collect();
    iddat_d.sort_unstable_by(|a, b| a.inchikey.cmp(&b.inchikey));
    iddat_d
}

fn read_name_f() -> Vec<(String, String)> {
    let file_path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("libs")
        .join("name_formu.txt");
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(false)
        .trim(csv::Trim::All)
        .from_path(file_path)
        .unwrap();
    let mut name_f: Vec<_> = rdr
        .records()
        .map(std::result::Result::unwrap)
        .map(|x| (x[0].to_owned(), x[1].to_owned()))
        .collect();
    name_f.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    name_f
}
