// SPDX-License-Identifier: GPL-3.0-or-later

use std::error::Error;
use std::path::Path;

use a7n_platform::feedback::{Color, Landing, Size, Style, preview};
use serde::de::{DeserializeOwned, value::StrDeserializer};

fn parse<T: DeserializeOwned>(value: &str) -> Result<T, serde::de::value::Error> {
    T::deserialize(StrDeserializer::new(value))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "Usage: fx_preview <style> [color] [size] [out_dir]\nStyles: glow, beam, aperture, sparks, membrane, flow\nSizes: small, medium, large\nDefaults: accent medium target/fx-preview\nColors: {}",
            Color::choices()
                .map(|(_, key, _)| key)
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Ok(());
    }
    if args.len() > 4 {
        return Err("expected <style> [color] [size] [out_dir]; see --help".into());
    }
    let style = parse::<Style>(&args[0])?;
    let color = parse::<Color>(args.get(1).map_or("accent", String::as_str))?;
    let size = parse::<Size>(args.get(2).map_or("medium", String::as_str))?;
    let out_dir = Path::new(args.get(3).map_or("target/fx-preview", String::as_str));
    preview::sheet(style, color, size, Landing::Ring, out_dir)?;
    println!(
        "Wrote {}-{{departure,arrival,strip}}.png in {}",
        format!("{style:?}").to_ascii_lowercase(),
        out_dir.display()
    );
    Ok(())
}
