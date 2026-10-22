


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::env::set_current_dir(std::env::current_exe().unwrap().parent().unwrap()).unwrap();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![mzml_tsv, tar_list, get_ms1])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

use std::error::Error;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;

const MISCDIR: &str = "misc";
#[tauri::command]
fn mzml_tsv() -> tauri::ipc::Response {
    let file_path = Path::new(MISCDIR).join("file_order.txt");
    tauri::ipc::Response::new(std::fs::read(file_path).unwrap())
}
macro_rules! unpack {
    ($sn:ident, $sn1:ident) => {
        fn $sn1(file: &mut BufReader<File>) -> io::Result<$sn> {
            let mut buffer = [0; std::mem::size_of::<$sn>()];
            file.read_exact(&mut buffer)?;
            Ok($sn::from_le_bytes(buffer))
        }
    };
}
unpack!(f32, unpack_f32);
unpack!(u32, unpack_u32);
unpack!(u16, unpack_u16);
unpack!(u8, unpack_u8);

fn unpack_string(file: &mut BufReader<File>) -> Result<String, Box<dyn Error>> {
    let mut str_buf = Vec::new();
    file.read_until(b'\0', &mut str_buf)?;
    str_buf.pop().ok_or("unpack_string")?;
    Ok(String::from_utf8(str_buf)?)
}
fn unpack_f32_2(file: &mut BufReader<File>) -> io::Result<(f32, f32)> {
    let mut buffer = [0; std::mem::size_of::<f32>()];
    file.read_exact(&mut buffer)?;
    let a = f32::from_le_bytes(buffer);
    file.read_exact(&mut buffer)?;
    Ok((a, f32::from_le_bytes(buffer)))
}
struct Param {
    i_mz: f32,
    ms1tol: f32,
    ms2tol: f32,
    win_s: Vec<(f32, f32)>,
}
fn read_param() -> Param {
    let file_path = Path::new(MISCDIR).join("param.bin");
    let bufw = &mut BufReader::new(File::open(file_path).unwrap());
    Param {
        i_mz: unpack_f32(bufw).unwrap(),
        ms1tol: unpack_f32(bufw).unwrap(),
        ms2tol: unpack_f32(bufw).unwrap(),
        win_s: {
            let len0 = unpack_u8(bufw).unwrap();
            (0..len0).map(|_| unpack_f32_2(bufw).unwrap()).collect()
        },
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Co {
    mz: f32,
    rt: f32,
    sc: f32,
    auc: f32,
    shape: f32,
    smooth: f32,
    pres: bool,
    cor_l: Vec<(f32, f32)>,
}
#[derive(serde::Serialize)]
struct Analyte {
    name: String,
    f_mz_l: Vec<(f32, f32)>,
    mz_rt_sc: Vec<Co>,
}

#[tauri::command]
fn tar_list(nfiles: usize) -> Vec<Analyte> {
    tar_list_(nfiles).unwrap()
}
fn tar_list_(nfiles: usize) -> Result<Vec<Analyte>, Box<dyn Error>> {
    let file_path = Path::new(crate::MISCDIR).join("analyte.bin");
    let buf = &mut BufReader::new(File::open(file_path)?);
    let mut ana_l = Vec::new();
    while let Ok(name) = unpack_string(buf) {
        buf.seek_relative(4)?;
        let len = unpack_u16(buf)?;
        let f_mz_l = (0..len)
            .map(|_| unpack_f32_2(buf))
            .collect::<Result<_, _>>()?;
        let mz_rt_sc = (0..nfiles)
            .map(|_| {
                let pres = unpack_u8(buf)? == 1;
                let rt = unpack_f32(buf)?;
                let sc = unpack_f32(buf)?;
                let mz = unpack_f32(buf)?;
                let (auc, shape, smooth, cor_l) = if pres {
                    (
                        unpack_f32(buf)?,
                        unpack_f32(buf)?,
                        unpack_f32(buf)?,
                        (0..len)
                            .map(|_| unpack_f32_2(buf))
                            .collect::<Result<_, _>>()?,
                    )
                } else {
                    (0., 0., 0., Vec::new())
                };
                Ok(Co {
                    mz,
                    rt,
                    sc,
                    auc,
                    shape,
                    smooth,
                    pres,
                    cor_l,
                })
            })
            .collect::<Result<_, Box<dyn Error>>>()?;
        ana_l.push(Analyte {
            name,
            f_mz_l,
            mz_rt_sc,
        });
    }
    Ok(ana_l)
}

#[tauri::command]
fn get_ms1(
    bn: &str,
    mz_rt_sc: Co,
    rtwid: f32,
    f_mz_l: Vec<f32>,
) -> (Vec<(f32, f32)>, Vec<Vec<(f32, f32)>>) {
    let Co {
        mz: ms1mz,
        rt: ms1rt,
        pres,
        ..
    } = mz_rt_sc;
    let param_t = read_param();
    let mut ms1_scans = Vec::new();
    let file_path = Path::new(MISCDIR).join(format!("ms1_{bn}.bin"));
    let buf = &mut BufReader::new(File::open(file_path).unwrap());
    buf.skip_until(b'\0').unwrap();
    let ms1tol = if pres { param_t.i_mz } else { param_t.ms1tol };
    while let Ok(rt) = unpack_f32(buf) {
        let len0 = unpack_u32(buf).unwrap();
        if rt < ms1rt - rtwid {
            buf.seek_relative(i64::from(len0) * 8).unwrap();
        } else if rt < ms1rt + rtwid {
            let pos0 = ms1mz - ms1tol;
            let pos1 = ms1mz + ms1tol;
            let mut ii = 0;
            ms1_scans.push((
                rt,
                (0..len0)
                    .map(|i| {
                        ii = i;
                        unpack_f32_2(buf).unwrap()
                    })
                    .skip_while(|x| x.0 <= pos0)
                    .take_while(|x| x.0 < pos1)
                    .map(|x| x.1)
                    .reduce(f32::max)
                    .unwrap_or(0.),
            ));
            if len0 > 1 {
                buf.seek_relative(i64::from(len0 - ii - 1) * 8).unwrap();
            }
        } else {
            break;
        }
    }
    (
        ms1_scans,
        get_ms2(bn, ms1mz, ms1rt, rtwid, f_mz_l, &param_t),
    )
}

fn get_ms2(
    bn: &str,
    ms1mz: f32,
    ms1rt: f32,
    rtwid: f32,
    f_mz_l: Vec<f32>,
    param_t: &Param,
) -> Vec<Vec<(f32, f32)>> {
    let mut sel_w = &(0f32, 0f32);
    let mut prev = 0.0;
    for s_w in param_t.win_s.iter().filter(|x| x.0 < ms1mz && ms1mz < x.1) {
        let cur = (ms1mz - s_w.0).min(s_w.1 - ms1mz);
        if cur > prev {
            sel_w = s_w;
            prev = cur;
        }
    }

    let mut ms2_scans = Vec::<(f32, Vec<_>)>::new();
    let file_path = Path::new(MISCDIR).join(format!("ms2_{bn}.bin"));
    let buf = &mut BufReader::new(File::open(file_path).unwrap());
    while let Ok(ms1mz) = unpack_f32(buf) {
        if sel_w.0 < ms1mz && ms1mz < sel_w.1 {
            let rt = unpack_f32(buf).unwrap();
            let len0 = unpack_u32(buf).unwrap();
            ms2_scans.push((rt, (0..len0).map(|_| unpack_f32_2(buf).unwrap()).collect()));
        } else {
            buf.seek_relative(4).unwrap();
            let len0 = unpack_u32(buf).unwrap();
            buf.seek_relative(i64::from(len0) * 8).unwrap();
        }
    }
    f_mz_l
        .iter()
        .map(|f_mz| {
            let rt_0 = ms1rt - rtwid;
            let rt_1 = ms1rt + rtwid;
            let f_mz_0 = f_mz - param_t.ms2tol;
            let f_mz_1 = f_mz + param_t.ms2tol;
            ms2_scans[ms2_scans.partition_point(|y| y.0 < rt_0)..]
                .iter()
                .take_while(|x| x.0 < rt_1)
                .map(|sc| {
                    (
                        sc.0,
                        sc.1[sc.1.partition_point(|y| y.0 < f_mz_0)..]
                            .iter()
                            .take_while(|y| y.0 < f_mz_1)
                            .map(|y| y.1)
                            .reduce(f32::max)
                            .unwrap_or(0.),
                    )
                })
                .collect()
        })
        .collect()
}
