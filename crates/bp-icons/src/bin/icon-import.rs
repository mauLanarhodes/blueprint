use bp_icons::{import_zip, install_pack};
use bp_model::CloudProvider;
use std::io::Read;
use std::path::PathBuf;

const USAGE: &str = "Usage: icon-import --provider aws|azure --version VERSION --input official-pack.zip --output ICON_DIRECTORY\n\nDownload the original pack from the provider, then import it locally. No icons are downloaded or modified.";

fn main() {
    if let Err(error) = run() {
        eprintln!("icon-import: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut provider = None;
    let mut version = None;
    let mut input = None;
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if matches!(flag.as_str(), "--help" | "-h") {
            println!("{USAGE}");
            return Ok(());
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}\n{USAGE}"))?;
        match flag.as_str() {
            "--provider" if provider.is_none() => {
                provider = Some(match value.as_str() {
                    "aws" => CloudProvider::Aws,
                    "azure" => CloudProvider::Azure,
                    _ => {
                        return Err(
                            format!("unknown provider {value:?}; choose aws or azure").into()
                        );
                    }
                })
            }
            "--version" if version.is_none() => version = Some(value),
            "--input" if input.is_none() => input = Some(PathBuf::from(value)),
            "--output" if output.is_none() => output = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown or repeated argument {flag:?}\n{USAGE}").into()),
        }
    }
    let provider = provider.ok_or(USAGE)?;
    let version = version.ok_or(USAGE)?;
    let input = input.ok_or(USAGE)?;
    let output = output.ok_or(USAGE)?;
    let mut bytes = Vec::new();
    std::fs::File::open(input)?
        .take((bp_icons::MAX_ZIP_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > bp_icons::MAX_ZIP_BYTES {
        return Err("icon ZIP exceeds 512 MiB".into());
    }
    let pack = import_zip(&bytes, provider, &version)?;
    let path = install_pack(&output, &pack)?;
    println!(
        "Installed {} {} icons (version {}) at {}",
        pack.icons.len(),
        provider.label(),
        version,
        path.display()
    );
    for warning in &pack.warnings {
        eprintln!("{warning}");
    }
    Ok(())
}
