use anyhow::{Context, Result};
use noisy_float::prelude::*;
use std::{borrow::Cow, collections::HashMap, fmt::Write as _, hash::Hash, io::Write};

use crate::m5::{M5File, PlateBlock, ReadInfo, Wavelength, WellRC};

pub(crate) fn write_csv(file: M5File, wtr: Box<dyn Write>) -> Result<()> {
    const HEADER: &[&str] = &[
        "Plate",
        "Well",
        "Row",
        "Col",
        "Time [hr]",
        "Temperature [C]",
        "Read Mode",
        "Excitation [nm]",
        "Emission [nm]",
        "Wavelength Description",
        "Value",
    ];

    let mut wtr = csv::Writer::from_writer(wtr);
    let mut cache = Cache::new(); // todo: move up to write_csv

    wtr.write_record(HEADER)
        .context("writing output CSV header")?;

    file.0
        .into_iter()
        .try_for_each(|block| write_block(block, &mut wtr, &mut cache))
        .context("writing CSV data")
}

#[derive(Debug)]
struct Cache {
    wellname: HashMap<WellRC, String>,
    time: HashMap<R64, String>,
    temp: HashMap<R64, String>,
    wl: HashMap<Wavelength, WaveStrings>,
}

impl Cache {
    fn new() -> Self {
        Self {
            wellname: HashMap::with_capacity(384),
            time: HashMap::with_capacity(4),
            temp: HashMap::with_capacity(4),
            wl: HashMap::with_capacity(4),
        }
    }
}

fn get_from<K, V, F>(map: &mut HashMap<K, V>, key: K, default: F) -> &V
where
    K: Hash + Eq + Copy,
    F: FnOnce(K) -> V,
{
    map.entry(key).or_insert_with(|| default(key))
}

fn write_block<W: Write>(
    block: PlateBlock,
    wtr: &mut csv::Writer<W>,
    cache: &mut Cache,
) -> Result<()> {
    let mut value = String::with_capacity(64);
    let PlateBlock { settings, data } = block;

    for (read_info, wells) in data {
        for well in wells {
            let wellname = get_from(&mut cache.wellname, well.well, fmt_wellname);
            // todo: more than 384 well (AA)?
            let r = &wellname[..1];
            let c = wellname[1..].trim_matches('0');
            let time = get_read_time(&read_info, &mut cache.time);
            let temp = get_from(&mut cache.temp, read_info.temp, fmt_temp);
            let WaveStrings { mode, ex, em, desc } =
                get_from(&mut cache.wl, well.wavelength, WaveStrings::from);

            write!(&mut value, "{}", well.value)?;

            let row: [&str; 11] = [
                &settings.name,
                wellname,
                r,
                c,
                time,
                temp,
                mode,
                ex,
                em,
                desc,
                &value,
            ];
            wtr.write_record(&row).context("writing output row")?;

            value.clear();
        }
    }

    Ok(())
}

fn get_read_time<'a>(info: &ReadInfo, cache: &'a mut HashMap<R64, String>) -> &'a str {
    info.get_time()
        .map(move |t| get_from(cache, t, fmt_time).as_str())
        .unwrap_or("")
}

fn fmt_wellname(rc: WellRC) -> String {
    format!("{}{:02}", (b'A' + rc.0) as char, rc.1 + 1)
}

fn fmt_temp(temp: R64) -> String {
    format!("{}", temp)
}

fn fmt_time(t: R64) -> String {
    format!("{}", t)
}

#[derive(Debug)]
struct WaveStrings {
    mode: &'static str,
    ex: Cow<'static, str>,
    em: Cow<'static, str>,
    desc: String,
}

impl From<Wavelength> for WaveStrings {
    fn from(src: Wavelength) -> Self {
        let (mode, ex, em, desc) = match src {
            Wavelength::Absorbance(abs) => {
                ("Absorbance", "".into(), "".into(), format!("{}nm", abs))
            }
            Wavelength::Fluorescence(ex, em) => (
                "Fluorescence",
                ex.to_string().into(),
                em.to_string().into(),
                format!("ex {}nm / em {}nm", ex, em),
            ),
        };

        Self { mode, em, ex, desc }
    }
}
