mod common;
mod cwt;
mod fillall;
mod parse;
mod print_t;
mod readlib;
mod score;
mod table;
const MISCDIR: &str = "misc";
use rayon::prelude::*;
use std::error::Error;
struct Param {
    mzml_fs: Vec<std::path::PathBuf>,
    ispos: bool,
    lib: Vec<String>,
    ms1tol: f32,
    ms2tol: f32,
    ms2_score: f32,
    min_peaks: u8,
    rt_shift: f32,
    peak_w: (f32, f32),
    num_t: usize,
    win_s: Vec<(f32, f32)>,
    peak_shape: f32,
    sn_score: f32,
    rt_search: f32,
    ms1ms2: f32,
    impute_width: f32,
    i_rt: f32,
    i_mz: f32,
}

struct MzAucC {
    mz: f32,
    auc: f32,
    score: f32,
    s_n: f32,
}
struct Cpd {
    i: usize,
    ent_i: usize,
    rt: f32,
    sc: f32,
    shape: f32,
    smooth: f32,
    pre: MzAucC,
    mz_auc_c: Vec<MzAucC>,
}
struct Ms {
    rt: f32,
    mz_i: Vec<(f32, f32)>,
}
struct Msms {
    ms1mz: f32,
    rt: f32,
    mz_i_l: Vec<(f32, f32)>,
    ce: f32,
}
fn main() -> Result<(), Box<dyn Error>> {
    let param_t = common::read_param()?;
    let lib_ent = readlib::get_cpds(&param_t);
    let _ = std::fs::remove_dir_all(MISCDIR);
    std::fs::create_dir(MISCDIR)?;
    print_param(&param_t)?;
    rayon::ThreadPoolBuilder::new()
        .num_threads(param_t.num_t)
        .build_global()?;
    let all_feat = param_t
        .mzml_fs
        .par_iter()
        .map(|mzml_f| {
            let bn = mzml_f.file_name().unwrap().to_str().unwrap();
            parse::mzml(mzml_f).unwrap();
            let ms1_scans = common::get_ms1(bn).unwrap();
            cwt::cwt(bn, &ms1_scans, &lib_ent, &param_t).unwrap();
            println!("{bn}");
            let ms2_scans = common::get_ms2(bn).unwrap();
            score::score(&param_t, bn, &lib_ent, &ms1_scans, &ms2_scans)
        })
        .collect();
    let ana_l = table::print_tab(&param_t, &lib_ent, all_feat)?;
    fillall::fill(&param_t, &ana_l)?;
    print_t::print_tar(&param_t)?;
    Ok(())
}
fn print_param(param_t: &Param) -> Result<(), Box<dyn Error>> {
    use std::io::Write;
    let file_path = std::path::Path::new(MISCDIR).join("param.bin");
    let mut bufb = std::io::BufWriter::new(std::fs::File::create(file_path)?);
    bufb.write_all(&param_t.i_mz.to_le_bytes())?;
    bufb.write_all(&param_t.ms1tol.to_le_bytes())?;
    bufb.write_all(&param_t.ms2tol.to_le_bytes())?;
    bufb.write_all(&u8::try_from(param_t.win_s.len()).unwrap().to_le_bytes())?;
    for (lb, rb) in &param_t.win_s {
        bufb.write_all(&lb.to_le_bytes())?;
        bufb.write_all(&rb.to_le_bytes())?;
    }
    Ok(())
}
