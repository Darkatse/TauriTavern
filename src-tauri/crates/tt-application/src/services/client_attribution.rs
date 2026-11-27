const REFERER: &str = "https://tauritavern.github.io";
pub(crate) const TITLE: &str = "TauriTavern";

pub(crate) fn headers() -> [(&'static str, &'static str); 2] {
    [("HTTP-Referer", REFERER), ("X-Title", TITLE)]
}
