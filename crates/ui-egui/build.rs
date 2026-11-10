use std::path::Path;

/// The interface languages a window built without the `all-languages` feature speaks besides English: a page's (apps/bimcraft-web,
/// Arqiflow's), whose page decides the language. A language comes back to it by its catalog's name here (`"pt"`, `"zh_TW"`, as in
/// translations/ui), and the page then asks for it.
const LANGUAGES: &[&str] = &[];

fn main() -> Result<(), String> {
    let root = std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?;
    let out = std::env::var("OUT_DIR").map_err(|e| e.to_string())?;
    let languages = std::env::var_os("CARGO_FEATURE_ALL_LANGUAGES").is_none().then_some(LANGUAGES);
    bimcraft_i18n::catalog::compile(&Path::new(&root).join("../../translations/ui"), &Path::new(&out).join("catalogs.rs"), languages)
}
