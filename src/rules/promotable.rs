//! Ares `Promotable<T>::Read` (`Utilities/TemplateDef.h`), shared by the Ares
//! TechnoType reader and the Phobos Interceptor guard ranges.

/// `format` with `""` and a trailing dot stripped is the common key, which
/// sets all three ranks when it parses; then the `Rookie`, `Veteran` and
/// `Elite` keys each override one. `read` is the field's `Valueable` read
/// (`None` in: no current value); failures retain.
pub(crate) fn read_promotable<T: Copy>(
    format: &str,
    current: [T; 3],
    read: impl Fn(&str, Option<T>) -> Option<T>,
) -> [T; 3] {
    let common = format.replace("%s", "");
    let common = common.strip_suffix('.').unwrap_or(&common);
    let mut value = read(common, None).map_or(current, |all| [all; 3]);
    for (slot, rank) in ["Rookie", "Veteran", "Elite"].into_iter().enumerate() {
        let key = format.replace("%s", rank);
        if let Some(read) = read(&key, Some(value[slot])) {
            value[slot] = read;
        }
    }
    value
}
