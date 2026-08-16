//! Makes Cargo track the locale catalog read by rust-i18n's proc macro, so
//! editing a translation triggers a rebuild.

fn main() {
    println!("cargo:rerun-if-changed=locales");
}
