// Adapted from MetaboKit DIA (SLING, BSD-3-Clause); see LICENSE.
use regex::Regex;
use std::cmp::Ordering;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};

pub struct Ent {
    pub mmass: f32,
    pub name: String,
    pub mz_i: Vec<(f32, f32)>,
    pub adduct: String,
    #[allow(dead_code)] // Retained for compatibility with the original library format.
    pub charge: i8,
    pub rt: Option<f32>,
    pub inchik: String,
}

fn unpack_string(file: &mut BufReader<File>, len0: usize) -> Result<String, crate::Failure> {
    let mut buffer = vec![0; len0];
    file.read_exact(&mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}
fn read_bin(
    param_t: &crate::Param,
    ispos: bool,
    mut infile: &str,
    ent_vec: &mut Vec<Ent>,
) -> Result<(), crate::Failure> {
    let atlas_f = infile == "Atlas_filtered";
    if atlas_f {
        infile = "Atlas";
    }
    let infile = param_t
        .libs_dir
        .clone()
        .ok_or("A libs directory is required for built-in libraries")?
        .join(format!("{infile}_{}", if ispos { "pos" } else { "neg" }));
    let re = Regex::new(r"\d\d?:\d\d?").unwrap();
    let atlas_f = infile.file_stem().unwrap().to_str().unwrap() == "Atlas_pos" && atlas_f;
    print!("\"{}\" ", infile.file_stem().unwrap().display());
    let buf = &mut BufReader::new(File::open(infile)?);
    let current_sz = ent_vec.len();
    while let Ok(mmass) = crate::common::unpack_f32(buf) {
        let mut buffer = [0; std::mem::size_of::<i8>()];
        buf.read_exact(&mut buffer)?;
        let charge = i8::from_le_bytes(buffer);
        let len0 = crate::common::unpack_u8(buf)?;
        let name = unpack_string(buf, len0.into())?;
        let len0 = crate::common::unpack_u8(buf)?;
        let adduct = unpack_string(buf, len0.into())?;
        let len0 = crate::common::unpack_u8(buf)?;
        let inchik = unpack_string(buf, len0.into())?;
        let len0 = crate::common::unpack_u8(buf)?;
        let mz = (0..len0)
            .map(|_| crate::common::unpack_f32(buf))
            .collect::<Result<Vec<f32>, _>>()?;
        let i = (0..len0)
            .map(|_| crate::common::unpack_f32(buf))
            .collect::<Result<Vec<f32>, _>>()?;
        if atlas_f
            && !["CAR ", "VAE ", "CE ", "CL "]
                .iter()
                .any(|x| name.starts_with(x))
        {
            let matches: Vec<(u8, u8)> = re
                .find_iter(&name)
                .map(|x| x.as_str().split_once(':').unwrap())
                .map(|x| (x.0.parse().unwrap(), x.1.parse().unwrap()))
                .collect();
            let fa_filter = || matches.iter().any(|x| !(12..=26).contains(&x.0) || x.1 > 6);
            match matches.len().cmp(&1) {
                Ordering::Greater => {
                    if fa_filter() {
                        continue;
                    }
                }
                Ordering::Equal => {
                    if name.starts_with("LP") {
                        if fa_filter() {
                            continue;
                        }
                    } else if !(24..=52).contains(&matches[0].0) {
                        continue;
                    }
                }
                Ordering::Less => {}
            }
        }
        ent_vec.push(Ent {
            mmass,
            mz_i: {
                if atlas_f {
                    mz.into_iter()
                        .zip(i)
                        .filter(|x| x.0 < mmass - 18.1)
                        .collect()
                } else {
                    mz.into_iter().zip(i).collect()
                }
            },
            name,
            adduct,
            charge,
            rt: None,
            inchik,
        });
    }
    println!("{}", ent_vec.len() - current_sz);
    Ok(())
}

fn read_lib(x: &str, label: Option<&str>, ent_vec: &mut Vec<Ent>) -> Result<(), crate::Failure> {
    let libpath = x.strip_prefix("user ").unwrap().trim();
    print!("{libpath:?} ");
    let mut cursor = BufReader::new(File::open(libpath)?);
    let label = label.unwrap_or(libpath);
    let mut name = String::new();
    let mut inchik = String::new();
    let mut adduct = String::new();
    let mut rt = None;
    let mut mmass = 0.0f32;
    let mut charge = 1i8;
    let re = Regex::new(r"\[(.*)\](.)").unwrap();
    let mut line = String::new();
    let mut line0 = String::new();
    let mut mz_i: Vec<(f32, f32, bool)> = Vec::new();
    let current_sz = ent_vec.len();
    while cursor.read_line(&mut line)? > 0 {
        let Some((lsp0, lsp1)) = line.split_once(": ") else {
            line.clear();
            continue;
        };
        let lsp1 = lsp1.trim();
        match lsp0.trim().to_uppercase().as_str() {
            "NAME" => lsp1.clone_into(&mut name),
            "PRECURSORMZ" => mmass = lsp1.parse()?,
            "PRECURSORTYPE" | "PRECURSOR_TYPE" => {
                (adduct, charge) = re.captures(lsp1).map_or_else(
                    || (lsp1.to_owned(), 1),
                    |caps| (caps[1].to_owned(), caps[2].parse().unwrap_or(1)),
                );
                if !name.contains(&adduct) {
                    name = format!("{name} {adduct}");
                }
            }
            "RETENTIONTIME" => rt = lsp1.parse().ok(),
            "INCHIKEY" => lsp1.clone_into(&mut inchik),
            "NUM PEAKS" => {
                use std::fmt::Write;
                write!(&mut name, " ({label})")?;
                line0.clear();
                while cursor.read_line(&mut line0)? > 0 {
                    if line0.trim().is_empty() {
                        break;
                    }
                    let mut iter = line0.split_whitespace();
                    mz_i.push((
                        iter.next().ok_or("Missing library fragment m/z")?.parse()?,
                        iter.next()
                            .ok_or("Missing library fragment intensity")?
                            .parse()?,
                        iter.next().is_some_and(|x| x == "*"),
                    ));
                    line0.clear();
                }
                let mut mz_i: Vec<_> = if mz_i.iter().any(|x| x.2) {
                    mz_i.drain(..).filter(|x| x.2).map(|x| (x.0, x.1)).collect()
                } else {
                    mz_i.drain(..).map(|x| (x.0, x.1)).collect()
                };
                if !mmass.is_finite()
                    || rt.is_some_and(|v: f32| !v.is_finite())
                    || mz_i
                        .iter()
                        .any(|(m, i)| !m.is_finite() || !i.is_finite() || *i < 0.0)
                {
                    return Err(format!(
                        "Invalid mass, intensity or retention time in {libpath}: {name}"
                    )
                    .into());
                }
                mz_i.sort_unstable_by(|b, a| a.1.partial_cmp(&b.1).unwrap());
                ent_vec.push(Ent {
                    mmass,
                    name: std::mem::take(&mut name),
                    mz_i,
                    adduct: std::mem::take(&mut adduct),
                    charge,
                    rt,
                    inchik: std::mem::take(&mut inchik),
                });
            }
            _ => (),
        }
        line.clear();
    }
    println!("{}", ent_vec.len() - current_sz);
    Ok(())
}

fn read_mz_rt(x: &str, ent_vec: &mut Vec<Ent>) -> Result<(), crate::Failure> {
    let libpath = x.strip_prefix("csv ").unwrap().trim();
    print!("{libpath:?} ");
    let rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_path(libpath)?;
    let current_sz = ent_vec.len();
    for row in rdr.into_records() {
        let x = row?;
        if x.len() < 4 {
            return Err("DIA target CSV requires name, adduct, m/z and RT columns".into());
        }
        let mut name = x[0].to_owned();
        let adduct = x[1].to_owned();
        if !name.contains(&adduct) {
            name = format!("{name} {adduct}");
        }
        ent_vec.push(Ent {
            mmass: x[2].parse()?,
            name,
            mz_i: Vec::new(),
            adduct,
            charge: 1,
            rt: x[3].parse().ok(),
            inchik: String::new(),
        });
    }
    println!("{}", ent_vec.len() - current_sz);
    Ok(())
}

#[must_use]
pub fn get_cpds(param_t: &crate::Param) -> Result<Vec<Ent>, crate::Failure> {
    let &crate::Param { ispos, .. } = param_t;
    let crate::Param { lib: lib_types, .. } = param_t;
    let mut lib_ent = Vec::new();
    for g_lib in [
        "nist",
        "Atlas",
        "Atlas_filtered",
        "MSDIAL",
        "hmdb",
        "sling",
        "MassBank",
        "FiehnHILIC",
    ] {
        if lib_types
            .iter()
            .any(|x| x.to_lowercase() == g_lib.to_lowercase())
        {
            param_t.check()?;
            read_bin(param_t, ispos, g_lib, &mut lib_ent)?;
        }
    }
    for x in lib_types.iter().filter(|x| x.starts_with("user ")) {
        param_t.check()?;
        read_lib(
            x,
            param_t.library_labels.get(x).map(String::as_str),
            &mut lib_ent,
        )?;
    }
    for x in lib_types.iter().filter(|x| x.starts_with("csv ")) {
        read_mz_rt(x, &mut lib_ent)?;
    }
    println!("total: {}", lib_ent.len());
    for x in &mut lib_ent {
        if !x.mmass.is_finite() || x.rt.is_some_and(|r| !r.is_finite()) {
            return Err("Library masses and retention times must be finite".into());
        }
        x.mz_i = x
            .mz_i
            .drain(..)
            .filter(|y| y.0 < x.mmass - 0.3)
            .take(10)
            .collect();
    }
    lib_ent.sort_unstable_by(|a, b| a.mmass.partial_cmp(&b.mmass).unwrap());
    Ok(lib_ent)
}
