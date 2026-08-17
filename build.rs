//! Makes Cargo track the locale catalog read by rust-i18n's proc macro, so
//! editing a translation triggers a rebuild. On Windows, embeds the app icon
//! resource so the executable shows Hakata's icon in Explorer and the taskbar.

fn main() {
    println!("cargo:rerun-if-changed=locales");

    #[cfg(windows)]
    embed_resource::compile("resources/windows/hakata.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
