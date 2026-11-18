//! Number and text formatting shared by every area, the Spanish way
//! (decimal comma, a space before the percent sign).

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const MIB: f64 = 1024.0 * 1024.0;

/// "8,0", "16", "31": gigabytes without the unit.
pub fn gb(bytes: u64) -> String {
    let value = bytes as f64 / GIB;
    if value >= 10.0 { format!("{value:.0}") } else { decimal(value, 1) }
}

/// "300 MB" or "1,5 GB".
pub fn mem(bytes: u64) -> String {
    let mb = bytes as f64 / MIB;
    if mb >= 1024.0 { format!("{} GB", decimal(mb / 1024.0, 1)) } else { format!("{mb:.0} MB") }
}

/// "60" or "59,94".
pub fn fps(fps: f32) -> String {
    if (fps - fps.round()).abs() < 0.005 { format!("{fps:.0}") } else { decimal(fps as f64, 2) }
}

/// A number with a decimal comma: `decimal(4.56, 1)` is "4,6".
pub fn decimal(value: f64, digits: usize) -> String {
    format!("{value:.digits$}").replace('.', ",")
}

/// "8 %", "0,4 %", "0 %". Small values keep one decimal so 0,4 % is not shown as 0 %.
pub fn pct(value: f32) -> String {
    if value > 0.0 && value < 1.0 {
        // Never round a real loss down to "0,0 %".
        format!("{} %", decimal(f64::from(value.max(0.1)), 1))
    } else {
        format!("{value:.0} %")
    }
}

/// "12 Mbps" or "3,4 Mbps".
pub fn mbps(value: f64) -> String {
    if value >= 10.0 { format!("{value:.0} Mbps") } else { format!("{} Mbps", decimal(value, 1)) }
}

/// "1 programa" / "3 programas".
pub fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 { format!("1 {one}") } else { format!("{count} {many}") }
}

/// A hardware name as people say it: "Intel(R) Core(TM) i5-10400F CPU" becomes
/// "Intel Core i5-10400F CPU". Windows reports names with trademark marks that
/// only add noise on screen. Only for display: the classification in
/// `hardware::tiers` keeps reading the raw name.
pub fn product_name(raw: &str) -> String {
    const MARKS: [&str; 5] = ["(r)", "(tm)", "(c)", "®", "™"];
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    'scan: while let Some(c) = rest.chars().next() {
        for mark in MARKS {
            // `get` keeps this safe when the mark would end inside a multibyte char.
            if rest.get(..mark.len()).is_some_and(|head| head.eq_ignore_ascii_case(mark)) {
                rest = &rest[mark.len()..];
                continue 'scan;
            }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Breaks text into lines of at most `width` characters without cutting words.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let needed = current.chars().count() + word.chars().count() + usize::from(!current.is_empty());
        if needed > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(gb(8 * GIB as u64), "8,0");
        assert_eq!(gb(16 * GIB as u64), "16");
        assert_eq!(mem(1536 * MIB as u64), "1,5 GB");
        assert_eq!(mem(300 * MIB as u64), "300 MB");
    }

    #[test]
    fn rates_and_percentages() {
        assert_eq!(fps(60.0), "60");
        assert_eq!(fps(59.94), "59,94");
        assert_eq!(pct(8.4), "8 %");
        assert_eq!(pct(0.4), "0,4 %");
        assert_eq!(pct(0.01), "0,1 %");
        assert_eq!(pct(0.0), "0 %");
        assert_eq!(mbps(12.4), "12 Mbps");
        assert_eq!(mbps(3.44), "3,4 Mbps");
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "programa", "programas"), "1 programa");
        assert_eq!(plural(3, "programa", "programas"), "3 programas");
    }

    #[test]
    fn product_names_lose_trademark_marks() {
        assert_eq!(product_name("Intel(R) Core(TM) i5-10400F CPU @ 2.90GHz"), "Intel Core i5-10400F CPU @ 2.90GHz");
        assert_eq!(product_name("AMD Radeon(TM) Graphics"), "AMD Radeon Graphics");
        assert_eq!(product_name("NVIDIA GeForce® RTX™ 3060"), "NVIDIA GeForce RTX 3060");
        assert_eq!(product_name("intel(r) uhd (tm"), "intel uhd (tm");
        assert_eq!(product_name("  "), "");
        assert_eq!(product_name("Placa ñandú (TM)"), "Placa ñandú");
    }

    #[test]
    fn wraps_without_breaking_words() {
        assert_eq!(wrap("uno dos tres cuatro cinco", 9), vec!["uno dos", "tres", "cuatro", "cinco"]);
        assert!(wrap("", 10).is_empty());
    }
}
