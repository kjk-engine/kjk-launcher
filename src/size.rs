//! How a size in bytes is written for the player.

/// "250 MB", "12 KB" or "1 KB": megabytes from 1 MB up, kilobytes below
/// that, and "1 KB" for anything under 1 KB. A kilobyte is 1,000 bytes and
/// a megabyte 1,000,000 bytes.
pub fn size(bytes: u64) -> String {
    let kilobytes = (bytes as f64 / 1e3).round() as u64;
    if kilobytes < 1000 {
        format!("{} KB", kilobytes.max(1))
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

#[cfg(test)]
mod tests {
    use super::size;

    #[test]
    fn sizes_in_kilobytes_and_megabytes() {
        let cases = [
            (0, "1 KB"),
            (300, "1 KB"),
            (999, "1 KB"),
            (12_000, "12 KB"),
            (12_400, "12 KB"),
            (999_400, "999 KB"),
            (999_600, "1 MB"),
            (1_000_000, "1 MB"),
            (250_400_000, "250 MB"),
        ];
        for (bytes, expected) in cases {
            assert_eq!(size(bytes), expected, "{bytes}");
        }
    }
}
