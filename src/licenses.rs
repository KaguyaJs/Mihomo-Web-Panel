//! License notices travel inside the executable, including in single-file releases.
use rust_embed::RustEmbed;
use std::io::{self, Write};

#[derive(RustEmbed)]
#[folder = "third_party/licenses/"]
struct ThirdPartyLicenses;

pub fn write(output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "mihomo-web-panel {}\n", env!("CARGO_PKG_VERSION"))?;
    output.write_all(include_bytes!("../LICENSE"))?;
    writeln!(output, "\n\nTHIRD_PARTY_NOTICES.md\n")?;
    output.write_all(include_bytes!("../THIRD_PARTY_NOTICES.md"))?;
    let mut names: Vec<_> = ThirdPartyLicenses::iter().collect();
    names.sort();
    for name in names {
        writeln!(output, "\n\n--- third_party/licenses/{name} ---\n")?;
        let file = ThirdPartyLicenses::get(&name)
            .ok_or_else(|| io::Error::other("embedded license text is missing"))?;
        output.write_all(&file.data)?;
    }
    writeln!(output)
}
