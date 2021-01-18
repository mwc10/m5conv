use std::{io::BufRead, str::FromStr};

use crate::utils::rmap2;
use anyhow::{anyhow, bail, Context, Error, Result};
use noisy_float::prelude::*;

#[derive(Debug)]
pub(crate) struct M5File(pub(crate) Vec<PlateBlock>);

impl M5File {
    pub(crate) fn read_and_parse<R: BufRead>(mut rdr: R) -> Result<Self> {
        let mut buf = String::with_capacity(0x100);

        rdr.read_line(&mut buf).context("reading block count")?;
        let block_count = get_block_count(&buf).context("parsing initial blocks count")?;
        buf.clear();

        (0..block_count)
            .map(|i| {
                PlateBlock::from_rdr(&mut rdr, &mut buf)
                    .with_context(|| anyhow!("parsing block {}", i + 1))
            })
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

#[derive(Debug)]
pub(crate) struct PlateBlock {
    pub settings: PlateSettings,
    pub data: Vec<(ReadInfo, Vec<WellValue>)>,
}

impl PlateBlock {
    fn from_rdr(mut rdr: &mut dyn BufRead, buf: &mut String) -> Result<Self> {
        // read and parse plate settings row
        rdr.read_line(buf).context("reading plate info row")?;
        let settings = PlateSettings::parse(buf).context("parsing plate info")?;
        buf.clear();
        // read time / temp / col headers line
        // TODO: more validation of this row? The first column seems to change based on ReadType
        rdr.read_line(buf)
            .context("reading temp. and plate col header line")?;
        match buf.split('\t').nth(1) {
            Some("Temperature(°C)") => (),
            Some(unk) => bail!("Unknown/unsupported temperature unit: {}", unk),
            None => bail!("Couldn't read temperature and plate headers:\n{}", buf),
        }
        buf.clear();

        // read each single read of a plate
        let mut data = Vec::with_capacity(settings.info.reads);
        for i in 0..settings.info.reads {
            let read_output = parse_plate(&mut rdr, buf, &settings)
                .with_context(|| anyhow!("parsing plate read {}", i + 1))?;
            data.push(read_output)
        }
        buf.clear();

        rdr.read_line(buf).context("reading end block magic line")?;
        if buf.trim() != "~End" {
            bail!("Expected block end line, got \"{}\"", buf);
        }
        buf.clear();

        Ok(Self { settings, data })
    }
}

#[derive(Debug)]
pub(crate) struct PlateSettings {
    pub name: String,
    pub read_type: ReadType,
    pub read_mode: ReadMode,
    // read_pattern: String, WellScan Only [idx 10]
    info: PlateInfo,
}

impl PlateSettings {
    pub(crate) fn parse(s: &str) -> Result<Self> {
        let info = s.split('\t').map(str::trim).collect::<Vec<_>>();
        if info.len() < 6 {
            bail!("Missing basic plate setting info:\n{:#?}", info);
        }

        let name = info[1].to_string();
        let read_type = ReadType::from_str(info[4])?;
        let read_mode = ReadMode::from_str(info[5])?;
        let unique_data = &info[6..];
        let info = PlateInfo::from_text(read_type, read_mode, unique_data)
            .with_context(|| anyhow!("bad info? {:#?}", unique_data))?;

        Ok(Self {
            name,
            read_type,
            read_mode,
            info,
        })
    }
}

#[derive(Debug)]
struct PlateInfo {
    plate_size: u32,
    row_start: u8,
    row_span: u8,
    col_start: u8,
    col_span: u8,
    reads: usize,
    wavelengths: Vec<Wavelength>,
}

impl PlateInfo {
    fn from_text(read_type: ReadType, read_mode: ReadMode, keys: &[&str]) -> Result<Self> {
        let info = match (read_type, read_mode) {
            (ReadType::Endpoint, ReadMode::Absorbance) => {
                let reads = keys[2].parse()?;
                let row_start = keys[13].parse()?;
                let row_span = keys[14].parse()?;
                let col_start = keys[10].parse()?;
                let col_span = keys[11].parse()?;
                let plate_size = keys[12].parse()?;
                let wave_no = keys[8].parse()?;
                let wavelengths = keys[9]
                    .split_whitespace()
                    .take(wave_no)
                    .map(|s| s.parse().map(Wavelength::Absorbance))
                    .collect::<Result<_, _>>()?;

                Self {
                    plate_size,
                    row_start,
                    row_span,
                    col_start,
                    col_span,
                    reads,
                    wavelengths,
                }
            }
            (ReadType::Endpoint, ReadMode::Fluorescence)
            | (ReadType::WellScan, ReadMode::Fluorescence) => {
                let reads = keys[3].parse().context("read no")?;
                let row_start = keys[23].parse().context("row start")?;
                let row_span = keys[24].parse().context("row span")?;
                let col_start = keys[11].parse().context("col start")?;
                let col_span = keys[12].parse().context("col span")?;
                let plate_size = keys[13].parse().context("plate size")?;
                let wave_no = keys[9].parse().context("wave no")?;
                let exs = keys[14].split_whitespace();
                let ems = keys[10].split_whitespace();
                let wavelengths = exs
                    .zip(ems)
                    .take(wave_no)
                    .map(|(ex, em)| rmap2(ex.parse(), em.parse(), Wavelength::Fluorescence))
                    .collect::<Result<_, _>>()
                    .context("parsing ex/em wavelengths")?;

                Self {
                    plate_size,
                    row_start,
                    row_span,
                    col_start,
                    col_span,
                    reads,
                    wavelengths,
                }
            }
            _ => bail!(
                "Unsupported read type and read mode combination: {:?} {:?}",
                read_type,
                read_mode
            ),
        };

        Ok(info)
    }

    fn total_wells_read(&self) -> usize {
        self.row_span as usize * self.col_span as usize * self.wavelengths.len()
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum ReadType {
    Endpoint,
    WellScan,
}

impl FromStr for ReadType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "Well Scan" => Ok(Self::WellScan),
            "Endpoint" => Ok(Self::Endpoint),
            _ => Err(anyhow!("Unsupported M5 read type: {}", s)),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub(crate) enum ReadMode {
    Fluorescence,
    Absorbance,
}

impl FromStr for ReadMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Fluorescence" => Ok(Self::Fluorescence),
            "Absorbance" => Ok(Self::Absorbance),
            _ => Err(anyhow::anyhow!("Unsupported read mode: {}", s)),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct ReadInfo {
    pub temp: R64,
    pub unique: UniqueReadInfo,
}

impl ReadInfo {
    fn parse_cols(c1: &str, c2: &str, rtype: ReadType) -> Result<Self> {
        let unique = match rtype {
            ReadType::Endpoint => UniqueReadInfo::None,
            ReadType::WellScan => {
                let time = parse_time(c1).context("parsing time column")?;
                UniqueReadInfo::Time(time)
            }
        };

        let temp = c2.parse().map(r64).context("parsing temperature value")?;

        Ok(Self { temp, unique })
    }

    pub(crate) fn get_time(&self) -> Option<R64> {
        match self.unique {
            UniqueReadInfo::None => None,
            UniqueReadInfo::Time(time) => Some(time),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum UniqueReadInfo {
    None,
    Time(R64),
}

pub type WellRC = (u8, u8);
#[derive(Debug)]
pub(crate) struct WellValue {
    pub wavelength: Wavelength,
    /// zero-indexed (row, col)
    pub well: WellRC,
    pub value: f64,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub(crate) enum Wavelength {
    Fluorescence(u16, u16), // ex, em
    Absorbance(u16),
}

fn get_block_count(s: &str) -> Result<u16> {
    let mut it = s.split_whitespace().take(2);

    match (it.next(), it.next()) {
        (Some("##BLOCKS="), Some(b)) => Ok(b),
        _ => Err(anyhow!("Missing BLOCKS magic string")),
    }
    .and_then(|b| b.parse().map_err(Into::into))
}

fn parse_time(s: &str) -> Result<R64> {
    let mut it = s.splitn(3, ':');
    let h: f64 = it
        .next()
        .ok_or_else(|| anyhow!("No hours in time: {}, s"))
        .and_then(|h| h.parse().map_err(Into::into))?;
    let m: f64 = it
        .next()
        .ok_or_else(|| anyhow!("No minutes in time: {}, s"))
        .and_then(|m| m.parse().map_err(Into::into))?;
    let s: f64 = it
        .next()
        .map_or(Ok(0.0), |s| s.parse())
        .context("couldn't parse seconds")?;

    Ok(r64(h + (m / 60.0) + (s / (60.0 * 60.0))))
}

fn parse_plate(
    rdr: &mut dyn BufRead,
    buf: &mut String,
    settings: &PlateSettings,
) -> Result<(ReadInfo, Vec<WellValue>)> {
    let total_wells = settings.info.total_wells_read();
    let mut output = Vec::with_capacity(total_wells);
    let (total_rows, total_cols) = match settings.info.plate_size {
        384 => Ok((16, 24)),
        96 => Ok((8, 12)),
        _ => Err(anyhow!(
            "Unsupported plate size {}",
            settings.info.plate_size
        )),
    }?;

    let mut read_info = None;

    for r in 0..total_rows {
        buf.clear();
        rdr.read_line(buf)?;

        let mut line = buf.split('\t');

        let c1 = line
            .next()
            .ok_or_else(|| anyhow!("expected info col 1: {}", buf))?;
        let c2 = line
            .next()
            .ok_or_else(|| anyhow!("expected info col 2: {}", buf))?;
        if read_info.is_none() {
            read_info = Some(ReadInfo::parse_cols(c1, c2, settings.read_type)?);
        }

        // todo: just collect first...?
        let row_values: Vec<_> = line.collect();

        let values = row_values
            .chunks(total_cols + 1)
            .zip(settings.info.wavelengths.iter().copied())
            .flat_map(|(values, wavelength)| {
                let (values, _spacer) = values.split_at(total_cols);

                values
                    .iter()
                    .copied()
                    .map(str::trim)
                    .enumerate()
                    .filter(|(_, s)| !s.is_empty())
                    .map(move |(c, value)| {
                        value
                            .parse()
                            .context("parsing well value")
                            .map(|value| WellValue {
                                wavelength,
                                value,
                                well: (r, c as u8),
                            })
                    })
            });

        for val in values {
            let val = val.context("issue parsing well")?;
            output.push(val);
        }
    }

    buf.clear();
    rdr.read_line(buf)?;
    // TODO: check for spacer row

    let read_info = read_info.ok_or_else(|| anyhow!("never found read info"))?;

    Ok((read_info, output))
}
