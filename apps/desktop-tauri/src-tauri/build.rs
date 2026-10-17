use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    emit_build_date();
    tauri_build::build()
}

/// Expose the UTC build date as `CODEXBAR_BUILD_DATE` (YYYY-MM-DD) for the
/// About pane. `SOURCE_DATE_EPOCH` pins it for reproducible builds.
///
/// `tauri_build` registers its own rerun triggers, so Cargo would otherwise
/// keep this script's first date across later incremental builds. Rerunning
/// when the shell or `codexbar` sources change keeps the date current; those
/// changes recompile this crate anyway.
fn emit_build_date() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../../../rust/src");
    let seconds = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or(0)
        });
    let (year, month, day) = civil_from_days(seconds / 86_400);
    println!("cargo:rustc-env=CODEXBAR_BUILD_DATE={year:04}-{month:02}-{day:02}");
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}
