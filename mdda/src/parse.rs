use crate::MISCDIR;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, BufWriter, Read, Write};
use std::path::Path;

pub fn mzml(mzml_f: &Path) -> Result<(), Box<dyn Error>> {
    let bn = mzml_f
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("invalid mzML file name"))?;
    let reader = Reader::from_file(mzml_f)?;
    let mut bw1 = BufWriter::new(File::create(
        Path::new(MISCDIR).join(format!("ms1_{bn}.bin")),
    )?);
    let mut bw2 = BufWriter::new(File::create(
        Path::new(MISCDIR).join(format!("ms2_{bn}.bin")),
    )?);
    read_mzml(reader, &mut bw1, &mut bw2)?;
    bw1.flush()?;
    bw2.flush()?;
    Ok(())
}

#[derive(Default)]
struct Spectrum {
    level: Option<u8>,
    centroid: bool,
    rt: Option<f32>,
    precursor: Option<f32>,
    ce: f32,
    len: usize,
    mz: Option<Vec<f32>>,
    intensity: Option<Vec<f32>>,
}

#[derive(Default)]
struct BinaryArray {
    is_mz: Option<bool>,
    zlib: Option<bool>,
    float64: Option<bool>,
    encoded: Vec<u8>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn attribute(e: &BytesStart<'_>, name: &str) -> Result<Option<String>, Box<dyn Error>> {
    e.try_get_attribute(name)?
        .map(|a| {
            Ok(a.normalized_value(quick_xml::XmlVersion::Implicit1_0)?
                .into_owned())
        })
        .transpose()
}

fn required_attribute(e: &BytesStart<'_>, name: &str) -> Result<String, Box<dyn Error>> {
    attribute(e, name)?.ok_or_else(|| invalid(format!("missing {name} attribute")).into())
}

fn apply_param(
    param: &BytesStart<'_>,
    parent: &str,
    spectrum: &mut Spectrum,
    array: Option<&mut BinaryArray>,
) -> Result<(), Box<dyn Error>> {
    let Some(accession) = attribute(param, "accession")? else {
        return Ok(());
    };
    if let Some(array) = array {
        match accession.as_str() {
            "MS:1000514" => array.is_mz = Some(true),
            "MS:1000515" => array.is_mz = Some(false),
            "MS:1000523" => array.float64 = Some(true),
            "MS:1000521" => array.float64 = Some(false),
            "MS:1000574" => array.zlib = Some(true),
            "MS:1000576" => array.zlib = Some(false),
            _ => (),
        }
        return Ok(());
    }
    match (parent, accession.as_str()) {
        ("spectrum", "MS:1000511") => {
            spectrum.level = Some(required_attribute(param, "value")?.parse()?);
        }
        ("spectrum", "MS:1000127") => spectrum.centroid = true,
        ("spectrum", "MS:1000128") => spectrum.centroid = false,
        ("scan", "MS:1000016") => {
            let mut rt: f64 = required_attribute(param, "value")?.parse()?;
            match attribute(param, "unitAccession")?.as_deref() {
                Some("UO:0000010") => rt /= 60.0,
                Some("UO:0000031") | None => (),
                Some(unit) => {
                    return Err(invalid(format!("unsupported scan time unit {unit}")).into());
                }
            }
            spectrum.rt = Some(rt as f32);
        }
        ("selectedIon", "MS:1000744") => {
            spectrum.precursor = Some(required_attribute(param, "value")?.parse()?);
        }
        ("activation", "MS:1000045") => {
            spectrum.ce = required_attribute(param, "value")?.parse()?;
        }
        _ => (),
    }
    Ok(())
}

fn read_mzml<R: BufRead>(
    mut reader: Reader<R>,
    bw1: &mut impl Write,
    bw2: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    reader.config_mut().trim_text(true);
    reader.config_mut().expand_empty_elements = true;
    let mut groups = HashMap::<String, Vec<BytesStart<'static>>>::new();
    let mut group: Option<(String, Vec<BytesStart<'static>>)> = None;
    let mut stack = Vec::<String>::new();
    let mut spectrum: Option<Spectrum> = None;
    let mut array: Option<BinaryArray> = None;
    let mut buf = Vec::new();
    let mut decoded = Vec::new();
    let mut inflated = Vec::new();
    let mut run_seen = false;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                let parent = stack.last().map(String::as_str).unwrap_or_default();
                match e.local_name().as_ref() {
                    "referenceableParamGroup" => {
                        group = Some((required_attribute(&e, "id")?, Vec::new()));
                    }
                    "run" => {
                        if run_seen {
                            return Err(invalid("multiple mzML runs are not supported").into());
                        }
                        let timestamp = attribute(&e, "startTimeStamp")?.unwrap_or_default();
                        bw1.write_all(timestamp.as_bytes())?;
                        bw1.write_all(b"\0")?;
                        run_seen = true;
                    }
                    "spectrum" if parent == "spectrumList" => {
                        spectrum = Some(Spectrum {
                            len: required_attribute(&e, "defaultArrayLength")?.parse()?,
                            ..Spectrum::default()
                        });
                    }
                    "binaryDataArray" if spectrum.is_some() => {
                        array = Some(BinaryArray::default());
                    }
                    "cvParam" => {
                        if let Some((_, params)) = group.as_mut() {
                            params.push(e.clone().into_owned());
                        } else if let Some(spectrum) = spectrum.as_mut() {
                            apply_param(&e, parent, spectrum, array.as_mut())?;
                        }
                    }
                    "referenceableParamGroupRef" if spectrum.is_some() => {
                        let id = required_attribute(&e, "ref")?;
                        let params = groups.get(&id).ok_or_else(|| {
                            invalid(format!("unknown referenceable parameter group {id}"))
                        })?;
                        for param in params {
                            apply_param(param, parent, spectrum.as_mut().unwrap(), array.as_mut())?;
                        }
                    }
                    _ => (),
                }
                stack.push(e.local_name().as_ref().to_owned());
            }
            Event::Text(e) if stack.last().is_some_and(|tag| tag == "binary") => {
                if let Some(array) = array.as_mut() {
                    array.encoded.extend_from_slice(e.as_bytes());
                }
            }
            Event::CData(e) if stack.last().is_some_and(|tag| tag == "binary") => {
                if let Some(array) = array.as_mut() {
                    array.encoded.extend_from_slice(e.as_bytes());
                }
            }
            Event::End(e) => {
                match e.local_name().as_ref() {
                    "referenceableParamGroup" => {
                        if let Some((id, params)) = group.take() {
                            groups.insert(id, params);
                        }
                    }
                    "binaryDataArray" => {
                        if let (Some(mut array), Some(spectrum)) = (array.take(), spectrum.as_mut())
                        {
                            if let Some(is_mz) = array.is_mz {
                                let values = decode_bin(&mut array, &mut decoded, &mut inflated)?;
                                let target = if is_mz {
                                    &mut spectrum.mz
                                } else {
                                    &mut spectrum.intensity
                                };
                                if target.replace(values).is_some() {
                                    return Err(invalid("duplicate spectrum binary array").into());
                                }
                            }
                        }
                    }
                    "spectrum" => {
                        if let Some(spectrum) = spectrum.take() {
                            write_spectrum(spectrum, bw1, bw2)?;
                        }
                    }
                    _ => (),
                }
                stack.pop();
            }
            Event::Eof => {
                if !stack.is_empty() {
                    return Err(invalid("unexpected end of mzML file").into());
                }
                break;
            }
            _ => (),
        }
        buf.clear();
    }
    if !run_seen {
        return Err(invalid("missing mzML run").into());
    }
    Ok(())
}

fn write_spectrum(
    spectrum: Spectrum,
    bw1: &mut impl Write,
    bw2: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    let level = spectrum.level.ok_or_else(|| invalid("missing MS level"))?;
    if !matches!(level, 1 | 2) {
        return Ok(());
    }
    if !spectrum.centroid {
        return Err(invalid("profile mode detected; centroid spectra are required").into());
    }
    let rt = spectrum
        .rt
        .filter(|value| value.is_finite())
        .ok_or_else(|| invalid("missing or invalid scan start time"))?;
    let mz = spectrum.mz.unwrap_or_default();
    let intensity = spectrum.intensity.unwrap_or_default();
    if mz.len() != spectrum.len || intensity.len() != spectrum.len {
        return Err(invalid("binary array length does not match defaultArrayLength").into());
    }
    let peaks: Vec<_> = mz
        .into_iter()
        .zip(intensity)
        .filter(|x| x.1 > 0.0)
        .collect();
    let len = u32::try_from(peaks.len())?;
    let writer: &mut dyn Write = if level == 1 {
        bw1
    } else {
        let precursor = spectrum
            .precursor
            .filter(|value| value.is_finite())
            .ok_or_else(|| invalid("missing or invalid selected ion m/z"))?;
        bw2.write_all(&precursor.to_le_bytes())?;
        bw2
    };
    writer.write_all(&rt.to_le_bytes())?;
    if level == 2 {
        writer.write_all(&spectrum.ce.to_le_bytes())?;
    }
    writer.write_all(&len.to_le_bytes())?;
    for (mz, intensity) in peaks {
        writer.write_all(&mz.to_le_bytes())?;
        writer.write_all(&intensity.to_le_bytes())?;
    }
    Ok(())
}

fn decode_bin(
    array: &mut BinaryArray,
    decoded: &mut Vec<u8>,
    inflated: &mut Vec<u8>,
) -> io::Result<Vec<f32>> {
    use base64::Engine as _;
    let zlib = array
        .zlib
        .ok_or_else(|| invalid("binary compression not set"))?;
    let float64 = array
        .float64
        .ok_or_else(|| invalid("binary precision not set"))?;
    array.encoded.retain(|byte| !byte.is_ascii_whitespace());
    decoded.clear();
    base64::engine::general_purpose::STANDARD
        .decode_vec(&array.encoded, decoded)
        .map_err(|error| invalid(error.to_string()))?;
    let bytes = if zlib && !decoded.is_empty() {
        inflated.clear();
        flate2::bufread::ZlibDecoder::new(decoded.as_slice()).read_to_end(inflated)?;
        inflated.as_slice()
    } else {
        decoded.as_slice()
    };
    let width = if float64 { 8 } else { 4 };
    if bytes.len() % width != 0 {
        return Err(invalid(
            "binary array contains an incomplete floating-point value",
        ));
    }
    Ok(if float64 {
        bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| f64::from_le_bytes(*chunk) as f32)
            .collect()
    } else {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    fn cv(accession: &str, value: &str) -> String {
        format!(r#"<cvParam accession="{accession}" value="{value}"/>"#)
    }

    fn parse(xml: &str) -> Result<(Vec<u8>, Vec<u8>), Box<dyn Error>> {
        let (mut ms1, mut ms2) = (Vec::new(), Vec::new());
        read_mzml(Reader::from_str(xml), &mut ms1, &mut ms2)?;
        Ok((ms1, ms2))
    }

    fn document(groups: &str, run_attributes: &str, spectra: &str) -> String {
        format!(
            "<mzML>{groups}<run {run_attributes}><spectrumList>{spectra}</spectrumList></run></mzML>"
        )
    }

    fn scan(time: f32, unit: &str) -> String {
        format!(
            r#"<scanList><scan><cvParam accession="MS:1000016" value="{time}" unitAccession="{unit}"/></scan></scanList>"#
        )
    }

    fn spectrum(level: u8, time: f32, len: usize, extra: &str) -> String {
        format!(
            r#"<spectrum defaultArrayLength="{len}">{}{}{}{extra}</spectrum>"#,
            cv("MS:1000511", &level.to_string()),
            cv("MS:1000127", ""),
            scan(time, "UO:0000031")
        )
    }

    fn binary(values: &[f32], float64: bool, zlib: bool) -> String {
        let mut bytes: Vec<u8> = values
            .iter()
            .flat_map(|&value| {
                if float64 {
                    (value as f64).to_le_bytes().to_vec()
                } else {
                    value.to_le_bytes().to_vec()
                }
            })
            .collect();
        if zlib {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&bytes).unwrap();
            bytes = encoder.finish().unwrap();
        }
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn arrays(mz: &[f32], intensities: &[f32], float64: bool, zlib: bool) -> String {
        let mut xml = String::from("<binaryDataArrayList>");
        for (kind, values) in [("MS:1000514", mz), ("MS:1000515", intensities)] {
            xml.push_str(&format!(
                "<binaryDataArray>{}{}{}<binary>\n {} \n</binary></binaryDataArray>",
                cv(kind, ""),
                cv(if float64 { "MS:1000523" } else { "MS:1000521" }, ""),
                cv(if zlib { "MS:1000574" } else { "MS:1000576" }, ""),
                binary(values, float64, zlib)
            ));
        }
        xml.push_str("</binaryDataArrayList>");
        xml
    }

    fn record(time: f32, precursor_ce: Option<(f32, f32)>, peaks: &[(f32, f32)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        if let Some((precursor, _)) = precursor_ce {
            bytes.extend_from_slice(&precursor.to_le_bytes());
        }
        bytes.extend_from_slice(&time.to_le_bytes());
        if let Some((_, ce)) = precursor_ce {
            bytes.extend_from_slice(&ce.to_le_bytes());
        }
        bytes.extend_from_slice(&(peaks.len() as u32).to_le_bytes());
        for (mz, intensity) in peaks {
            bytes.extend_from_slice(&mz.to_le_bytes());
            bytes.extend_from_slice(&intensity.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn shared_parameters_seconds_missing_timestamp_and_empty_scan() {
        let groups = format!(
            r#"<referenceableParamGroupList>
            <referenceableParamGroup id="metadata">{}{}</referenceableParamGroup>
            <referenceableParamGroup id="mz">{}{}{}</referenceableParamGroup>
            <referenceableParamGroup id="intensity">{}{}{}</referenceableParamGroup>
            </referenceableParamGroupList>"#,
            cv("MS:1000511", "1"),
            cv("MS:1000127", ""),
            cv("MS:1000514", ""),
            cv("MS:1000523", ""),
            cv("MS:1000574", ""),
            cv("MS:1000515", ""),
            cv("MS:1000521", ""),
            cv("MS:1000576", "")
        );
        let mut scans = format!(
            r#"<spectrum defaultArrayLength="2">
            <referenceableParamGroupRef ref="metadata"/>{}<binaryDataArrayList>
            <binaryDataArray><referenceableParamGroupRef ref="mz"/><binary>{}</binary></binaryDataArray>
            <binaryDataArray><referenceableParamGroupRef ref="intensity"/><binary>{}</binary></binaryDataArray>
            </binaryDataArrayList></spectrum>"#,
            scan(90.0, "UO:0000010"),
            binary(&[101.5, 202.25], true, true),
            binary(&[15.0, 25.0], false, false)
        );
        scans.push_str(&format!(
            r#"<spectrum defaultArrayLength="0">
            <referenceableParamGroupRef ref="metadata"/>{}</spectrum>"#,
            scan(120.0, "UO:0000010")
        ));
        let (ms1, ms2) = parse(&document(&groups, "", &scans)).unwrap();
        let mut expected = vec![0];
        expected.extend(record(1.5, None, &[(101.5, 15.0), (202.25, 25.0)]));
        expected.extend(record(2.0, None, &[]));
        assert_eq!(ms1, expected);
        assert!(ms2.is_empty());
    }

    #[test]
    fn indexed_inline_metadata_preserves_timestamp_default_ce_and_peak_filter() {
        let ms1_scan = spectrum(
            1,
            2.5,
            3,
            &arrays(&[100.0, 200.0, 300.0], &[0.0, -1.0, 12.0], true, true),
        );
        let precursor = format!(
            "<precursorList><precursor><selectedIonList><selectedIon>{}</selectedIon></selectedIonList></precursor></precursorList>",
            cv("MS:1000744", "450.25")
        );
        let ms2_scan = spectrum(
            2,
            3.0,
            1,
            &(precursor + &arrays(&[75.0], &[20.0], false, false)),
        );
        let xml = format!(
            "<indexedmzML>{}</indexedmzML>",
            document(
                "",
                r#"startTimeStamp="2026-09-30T12:34:56Z""#,
                &(ms1_scan + &ms2_scan)
            )
        );
        let (ms1, ms2) = parse(&xml).unwrap();
        let mut expected = b"2026-09-30T12:34:56Z\0".to_vec();
        expected.extend(record(2.5, None, &[(300.0, 12.0)]));
        assert_eq!(ms1, expected);
        assert_eq!(ms2, record(3.0, Some((450.25, 0.0)), &[(75.0, 20.0)]));
    }

    #[test]
    fn all_float_widths_and_compression_modes_have_identical_cache_bytes() {
        for float64 in [false, true] {
            for zlib in [false, true] {
                let scan = spectrum(
                    1,
                    1.0,
                    2,
                    &arrays(&[42.5, 123.25], &[2.0, 8.0], float64, zlib),
                );
                let (ms1, ms2) = parse(&document("", "", &scan)).unwrap();
                let mut expected = vec![0];
                expected.extend(record(1.0, None, &[(42.5, 2.0), (123.25, 8.0)]));
                assert_eq!(ms1, expected, "float64={float64}, zlib={zlib}");
                assert!(ms2.is_empty());
            }
        }
    }

    #[test]
    fn malformed_and_truncated_xml_return_errors() {
        for xml in [
            "",
            "<mzML/>",
            "<mzML><run>",
            "<mzML><run></mzML>",
            "<mzML><run bad=\"unterminated>",
        ] {
            assert!(parse(xml).is_err(), "accepted malformed XML: {xml}");
        }
    }

    #[test]
    fn bad_references_and_inconsistent_array_lengths_return_errors() {
        let unknown = r#"<spectrum defaultArrayLength="0"><referenceableParamGroupRef ref="missing"/></spectrum>"#;
        assert!(
            parse(&document("", "", unknown))
                .unwrap_err()
                .to_string()
                .contains("unknown referenceable")
        );
        let mismatch = spectrum(1, 1.0, 2, &arrays(&[10.0], &[20.0], false, false));
        assert!(
            parse(&document("", "", &mismatch))
                .unwrap_err()
                .to_string()
                .contains("array length")
        );
        let profile = spectrum(1, 1.0, 0, "").replace("MS:1000127", "MS:1000128");
        assert!(
            parse(&document("", "", &profile))
                .unwrap_err()
                .to_string()
                .contains("profile mode")
        );
    }
}
