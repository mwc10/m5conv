use anyhow::{Result, Context};
use std::{collections::HashMap, io::Write, fmt::Write as _};
use noisy_float::prelude::*;

use crate::m5::{M5File, PlateBlock, ReadType, Wavelength, WellRC};

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


pub(crate) fn write_csv(file: M5File, wtr: Box<dyn Write>) -> Result<()> {
    let mut wtr = csv::Writer::from_writer(wtr);
    for block in file.0 {
        match block.settings.read_type {
            ReadType::Endpoint => write_endpoint(block, &mut wtr),
            ReadType::WellScan => todo!(),
        }?;
    }

    Ok(())
}

fn write_endpoint<W: Write>(block: PlateBlock, wtr: &mut csv::Writer<W>) -> Result<()> {
    const ENDPOINT_HEADER: &[&str] = &[
        "Plate",
        "Well",
        "Row",
        "Col",
        "Temperature [C]",
        "Read Mode",
        "Wavelength [nm]",
        "Value",
    ];
    wtr.write_record(ENDPOINT_HEADER).context("writing output CSV header")?;

    let mut value = String::with_capacity(64);
    let mut cache = Cache::new(); // todo: move up to write_csv

    let PlateBlock {settings, data} = block;
    for (read_info, wells) in data {
        for well in wells {
            let wellname = cache.wellname.entry(well.well).or_insert_with(|| fmt_wellname(well.well));
            // todo: > 384 well (AA)?
            let r = &wellname[..1];
            let c = wellname[1..].trim_matches('0');
            let temp = cache.temp.entry(read_info.temp).or_insert_with(|| format!("{}",read_info.temp));
            let (mode, wl) = cache.wl.entry(well.wavelength).or_insert_with(|| well.wavelength.as_strings());
            write!(&mut value, "{}", well.value)?;

            write!(&mut value, "{}", well.value)?;

            let row: [&str; 8] = [&settings.name, wellname, r, c, temp, mode, wl, &value];
            wtr.write_record(&row).context("writing output row")?;

            value.clear();
        }
    }

    Ok(())
}

const WELLSCAN_HEADER: &[&str] = &[
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

fn fmt_wellname(rc: WellRC) -> String {
    format!("{}{:02}", (b'A' + rc.0) as char, rc.1 + 1)
}

fn fmt_time(t: R64) -> String {
    format!("{}", t)
}