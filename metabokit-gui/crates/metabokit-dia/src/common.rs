// Adapted from MetaboKit DIA (SLING, BSD-3-Clause); see LICENSE.
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader, Read};

macro_rules! unpack {
    ($sn:ident, $sn1:ident) => {
        pub fn $sn1(file: &mut BufReader<File>) -> io::Result<$sn> {
            let mut buffer = [0; std::mem::size_of::<$sn>()];
            file.read_exact(&mut buffer)?;
            Ok($sn::from_le_bytes(buffer))
        }
    };
}
unpack!(f32, unpack_f32);
unpack!(u32, unpack_u32);
unpack!(u8, unpack_u8);
unpack!(u16, unpack_u16);

pub fn unpack_string(file: &mut BufReader<File>) -> Result<String, crate::Failure> {
    let mut str_buf = Vec::new();
    file.read_until(b'\0', &mut str_buf)?;
    str_buf.pop().ok_or("unpack_string")?;
    Ok(String::from_utf8(str_buf)?)
}
pub fn unpack_f32_2(file: &mut BufReader<File>) -> io::Result<(f32, f32)> {
    let mut buffer = [0; std::mem::size_of::<f32>()];
    file.read_exact(&mut buffer)?;
    let a = f32::from_le_bytes(buffer);
    file.read_exact(&mut buffer)?;
    Ok((a, f32::from_le_bytes(buffer)))
}

pub fn get_ms1(misc: &std::path::Path, bn: &str) -> Result<Vec<crate::Ms>, crate::Failure> {
    let mut ms1_scans = Vec::new();
    let file_path = misc.join(format!("ms1_{bn}.bin"));
    let buf = &mut BufReader::new(File::open(file_path)?);
    buf.skip_until(b'\0')?;
    while let Ok(rt) = unpack_f32(buf) {
        let len0 = unpack_u32(buf)?;
        ms1_scans.push(crate::Ms {
            rt,
            mz_i: (0..len0)
                .map(|_| unpack_f32_2(buf))
                .collect::<Result<_, _>>()?,
        });
    }
    Ok(ms1_scans)
}

pub fn get_ms2(misc: &std::path::Path, bn: &str) -> Result<Vec<crate::Msms>, crate::Failure> {
    let mut ms2_scans = Vec::new();
    let file_path = misc.join(format!("ms2_{bn}.bin"));
    let buf = &mut BufReader::new(File::open(file_path)?);
    while let Ok(ms1mz) = unpack_f32(buf) {
        let rt = unpack_f32(buf)?;
        let ce = unpack_f32(buf)?;
        let len0 = unpack_u32(buf)?;
        ms2_scans.push(crate::Msms {
            ms1mz,
            rt,
            mz_i_l: (0..len0)
                .map(|_| unpack_f32_2(buf))
                .collect::<Result<_, _>>()?,
            ce,
        });
    }
    Ok(ms2_scans)
}

#[must_use]
pub fn get_chrom2(
    cpd: (f32, f32, f32),
    scans: &[(f32, &Vec<(f32, f32)>)],
    tol: f32,
) -> Vec<(f32, f32)> {
    let (mz, rt, sc) = cpd;
    let rt0 = rt - sc;
    let rt1 = rt + sc;
    let mz0 = mz - tol;
    let mz1 = mz + tol;
    scans[scans.partition_point(|x| x.0 < rt0)..]
        .iter()
        .take_while(|x| x.0 < rt1)
        .map(|(rt, mz_i_l)| {
            let pos0 = mz_i_l.partition_point(|x| x.0 < mz0);
            (
                *rt,
                mz_i_l[pos0..]
                    .iter()
                    .take_while(|x| x.0 < mz1)
                    .map(|x| x.1)
                    .reduce(f32::max)
                    .unwrap_or(0.0),
            )
        })
        .collect()
}
#[must_use]
pub fn get_chrom(cpd: (f32, f32, f32), scans: &[crate::Ms], tol: f32) -> Vec<(f32, f32)> {
    let (mz, rt, sc) = cpd;
    let rt_l = rt - sc;
    let rt_r = rt + sc;
    let mz0 = mz - tol;
    let mz1 = mz + tol;
    scans[scans.partition_point(|x| x.rt < rt_l)..]
        .iter()
        .take_while(|x| x.rt < rt_r)
        .map(|sc| {
            let pos0 = sc.mz_i.partition_point(|x| x.0 < mz0);
            (
                sc.rt,
                sc.mz_i[pos0..]
                    .iter()
                    .take_while(|x| x.0 < mz1)
                    .map(|x| x.1)
                    .reduce(f32::max)
                    .unwrap_or(0.0),
            )
        })
        .collect()
}
#[must_use]
pub fn median(mut num_l: Vec<f32>) -> f32 {
    let pos = num_l.len() / 2;
    num_l.select_nth_unstable_by(pos, |a, b| a.partial_cmp(b).unwrap());
    if num_l.len() % 2 == 1 {
        num_l[pos]
    } else {
        num_l[pos].midpoint(num_l[..pos].iter().copied().reduce(f32::max).unwrap())
    }
}
