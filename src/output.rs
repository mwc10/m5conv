use anyhow::{Result, Context};
use std::{collections::HashMap, io::Write, fmt::Write as _};
use noisy_float::prelude::*;

use crate::{Wavelength, WellValue};

type WellRC = (u8, u8);
#[derive(Debug)]
struct Cache {
    wellname: HashMap<WellRC, String>,
    time: HashMap<R64, String>,
    temp: HashMap<R64, String>,
    wl: HashMap<Wavelength, (&'static str, String)>,
}

impl Cache {
    fn new() -> Self {
        Self {
            wellname: HashMap::with_capacity(384),
            // todo: maybe these will be known from parsing file?
            time: HashMap::with_capacity(4),
            temp: HashMap::with_capacity(4),
            wl: HashMap::with_capacity(4),
        }
    }
}

const HEADER: &[&str] = &[
    "Plate",
    "Well",
    "Row",
    "Col",
    "Time [hr]",
    "Temperature [C]",
    "Read Mode",
    "Wavelength",
    "Value",
];

pub(crate) fn write_data(data: Vec<WellValue>, wtr: Box<dyn Write>) -> Result<()> {
    let mut wtr = csv::Writer::from_writer(wtr);
    wtr.write_record(HEADER).context("writing output CSV header")?;

    let mut value = String::with_capacity(64);
    let mut cache = Cache::new();
    for well in data {
        let name = cache.wellname.entry(well.well).or_insert_with(|| fmt_wellname(well.well));
        // todo: > 384 well (AA)?
        let r = &name[..1];
        let c = name[1..].trim_matches('0');
        let time = cache.time.entry(well.time).or_insert_with(|| format!("{}",well.time));
        let temp = cache.temp.entry(well.temp).or_insert_with(|| format!("{}",well.temp));
        let (mode, settings) = cache.wl.entry(well.wavelength).or_insert_with(|| well.wavelength.as_strings());

        write!(&mut value, "{}", well.value)?;

        let row = [well.plate, name, r, c, time, temp, mode, settings, &value];
        wtr.write_record(&row).context("writing output row")?;

        value.clear();
    }


    Ok(())
}

fn fmt_wellname(rc: WellRC) -> String {
    format!("{}{:02}", (b'A' + rc.0) as char, rc.1 + 1)
}