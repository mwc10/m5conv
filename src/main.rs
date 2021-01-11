use anyhow::{self, bail, Context};
use encoding_rs::MACINTOSH;
use encoding_rs_io::DecodeReaderBytesBuilder;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::{env, str::FromStr};
use noisy_float::prelude::*;

mod m5;
mod output;


fn print_usage() {
    println!("{} {}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"));
    println!("Convert Softmax M5(e) tab-delimited to flat CSV by well");
    println!();
    println!("Usage:");
    println!("  {} <input> [output]", env!("CARGO_BIN_NAME"));
    println!();
    println!("  input           path to M5 tsv file");
    println!("  [output]        path to output, or stdout if not present");
}

enum Args {
    Help,
    Missing,
    Convert(PathBuf, Box<dyn Write>),
}

impl Args {
    fn from_env() -> anyhow::Result<Self> {
        let mut args = env::args().skip(1);
        let input = args.next();
        let output = args.next();

        match input {
            Some(s) if s == "-h" || s == "--help" => Ok(Self::Help),
            None => Ok(Self::Missing),
            Some(p) => {
                let input = PathBuf::from(p);
                let output = match output {
                    Some(p) => {
                        let f = File::create(PathBuf::from(p)).context("creating output file")?;
                        Box::new(BufWriter::new(f)) as Box<dyn Write>
                    }
                    None => Box::new(io::stdout()) as Box<dyn Write>,
                };
                Ok(Self::Convert(input, output))
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::from_env().context("parsing args")?;

    match args {
        Args::Help => print_usage(),
        Args::Missing => {
            eprintln!("Missing input M5 tab-delimited file");
            eprintln!("Pass --help for more info");
        },
        Args::Convert(input, output) => {
            parse_input(input, output)?;
        }
    }

    Ok(())
}

#[derive(Debug)]
struct M5Data {
    blocks: u16,
}

fn parse_input<'a>(path: PathBuf, output: Box<dyn Write>) -> anyhow::Result<()> {
    // output text file seems to be in macroman encoding..? Just for the degree symbol...
    let decoder = DecodeReaderBytesBuilder::new()
        .encoding(Some(MACINTOSH))
        .build(File::open(path)?);
    let mut rdr = BufReader::new(decoder);
    let mut buf = String::with_capacity(0x100);

    rdr.read_line(&mut buf).context("reading block count")?;
    let block_count = m5::M5DataBlocks::from_str(&buf).context("parsing initial blocks count")?;
    //println!("{:?}", block_count);
    buf.clear();

    // loop for multiple blocks here?
    rdr.read_line(&mut buf).context("reading plate info")?;
    let settings = parse_settings(&buf).context("parsing plate info")?;
    //println!("{:#?}", settings);
    buf.clear();

    rdr.read_line(&mut buf)
        .context("reading temp. and plate col header line")?;
    match buf.split('\t').nth(1) {
        Some("Temperature(°C)") => (),
        Some(unk) => bail!("Unknown/unsupported temperature unit: {}", unk),
        None => bail!("Couldn't read temperature and plate headers:\n{}", &buf),
    }
    buf.clear();

    let plate_data = parse_plate(&mut rdr, &mut buf, &settings)?;

    //write_data(plate_data, output).context("writing output CSV")
    output::write_data(plate_data, output).context("writing output CSV")
}

fn parse_plate<'p, R: BufRead>(
    mut rdr: R,
    buf: &mut String,
    settings: &'p PlateSettings,
) -> anyhow::Result<Vec<WellValue<'p>>> {
    let total_wells =
        settings.wavelengths.len() * (settings.cols as usize) * (settings.rows as usize);
    let mut output = Vec::with_capacity(total_wells);
    //let mut plate_buf = Vec::with_capacity(plate_cols);

    let mut time = None;
    let mut temp = None;

    // loop for multiple time points here?
    for r in 0..settings.rows {
        //plate_buf.clear();
        buf.clear();
        rdr.read_line(buf)?;

        let mut line = buf.split('\t');
        let new_time = line
            .next()
            .ok_or(anyhow::anyhow!("no time column"))
            .and_then(get_time)
            .context(format!("couldn't parse time, {}", &buf))?;
        let new_temp = line
            .next()
            .ok_or(anyhow::anyhow!("no temp column"))
            .and_then(get_temp)
            .context("couldn't parse temperaute")?;

        time = time.or(new_time);
        temp = temp.or(new_temp);

        // borrow checker
        let plate_buf: Vec<_> = line.collect();
        //let plate_iter =

        plate_buf
            .chunks(settings.cols as usize + 1)
            .zip(settings.wavelengths.iter().copied())
            .flat_map(|(plate, wavelength)| {
                let (plate, spacer) = plate.split_at(settings.cols as usize);

                plate
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
                                plate: &settings.name,
                                temp: temp.expect("temp value before wells"),
                                time: time.expect("time value before wells"),
                                wavelength,
                                well: (r, c as u8),
                                value,
                            })
                    })
            })
            .try_for_each::<_, anyhow::Result<()>>(|val| {
                let val = val.context("issue parsing well value")?;

                output.push(val);

                Ok(())
            })?;
    }

    buf.clear();
    rdr.read_line(buf)?;
    // check that this line is empty
    //println!("empty line?\n'{}'", &buf);

    //println!("Time: {:?}hr and Temp: {:?}C", time, temp)

    //line.next().ok_or(anyhow::anyhow!("missing spacer column")).and_then(check_spacer)?;

    Ok(output)
}

fn check_spacer(s: &str) -> anyhow::Result<()> {
    if s.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("non-empty spacer column: {}", s))
    }
}

fn get_temp(s: &str) -> anyhow::Result<Option<R64>> {
    if s.is_empty() {
        Ok(None)
    } else {
        Some(s.parse::<f64>().map(r64).map_err(Into::into)).transpose()
    }
}

fn get_time(s: &str) -> anyhow::Result<Option<R64>> {
    if s.is_empty() {
        Ok(None)
    } else {
        Some(parse_time(s)).transpose()
    }
}

fn parse_time(s: &str) -> anyhow::Result<R64> {
    let mut it = s.splitn(2, ':');
    let h: f64 = it
        .next()
        .ok_or_else(|| anyhow::anyhow!("No hours in time: {}, s"))
        .and_then(|h| h.parse().map_err(Into::into))?;
    let m: f64 = it
        .next()
        .ok_or_else(|| anyhow::anyhow!("No minutes in time: {}, s"))
        .and_then(|m| m.parse().map_err(Into::into))?;

    Ok(r64(h + (m / 60.0)))
}

fn parse_settings(info: &str) -> anyhow::Result<PlateSettings> {
    let info = info.split('\t').map(str::trim).collect::<Vec<_>>();

    // todo: probably less, check for < 12 first for initial info
    // then check for more for specific things (F vs ABS, eg)
    if info.len() < 31 {
        anyhow::bail!(
            "Less plate info entries than expected ({} < 31)",
            info.len()
        );
    }

    let name = info[1].to_string();
    let read_type = info[4].to_string();
    let read_mode = ReadMode::from_str(info[5])?;
    let reads = info[9].parse()?;
    let pattern = info[10].to_string();

    // sub function based on read_mode
    let waveno: usize = info[15].parse()?;
    let ems = info[20].split_whitespace();
    let exs = info[16].split_whitespace();
    let wavelengths = ems
        .zip(exs)
        .take(waveno)
        .map(|(em, ex)| {
            em.parse()
                .and_then(|em| ex.parse().map(|ex| Wavelength::Fluorescence(em, ex)))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let wells = info[19].parse()?;
    let rows = info[30].parse()?;
    let cols = info[18].parse()?;

    let settings = PlateSettings {
        name,
        rows,
        cols,
        wells,
        wavelengths,
        reads,
        read_mode,
        read_type,
        pattern,
    };

    Ok(settings)
}

// TODO: platesettings {basic, read_info, plate_info}
#[derive(Debug)]
struct PlateSettings {
    name: String,
    rows: u8,
    cols: u8,
    wells: u32,
    wavelengths: Vec<Wavelength>,
    reads: u32, // timepoints, probably
    read_mode: ReadMode,
    read_type: String,
    pattern: String,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ReadMode {
    Fluorescence,
    //Absorbance,
}

impl FromStr for ReadMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Fluorescence" => Ok(Self::Fluorescence),
            //"Absorbance" => Ok(Self::Absorbance),
            _ => Err(anyhow::anyhow!("Unknown read mode: {}", s)),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
enum Wavelength {
    Fluorescence(u16, u16), // ex, em
                            //Absorbance(u16),
}

impl Wavelength {
    fn as_strings(&self) -> (&'static str, String) {
        match self {
            Self::Fluorescence(ex, em) => ("Fluorescence", format!("ex {}/em {}", ex, em))
        }
    }
}

#[derive(Debug)]
struct WellValue<'a> {
    plate: &'a str,
    /// for now, only deg. Celcius
    temp: R64,
    /// hours
    time: R64,
    wavelength: Wavelength,
    /// zero-indexed (row, col)
    well: (u8, u8),
    value: f64,
}
