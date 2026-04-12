use crate::util::extract_cstring;
use crate::vst::Vst3Library;
use std::fs;
use std::path::{Path, PathBuf};
use vst3::Steinberg::kResultOk;

#[derive(Debug, Clone)]
#[allow(unused)]
pub struct PluginDescriptor {
    pub name: String,
    pub category: String,
    pub format: PluginFormat,
    pub is_instrument: bool,
    pub library_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginFormat {
    Vst3,
    // Clap, Vst2, Au, etc.
}

use vst3::ComRef;
use vst3::Steinberg::{
    IPluginFactory2, IPluginFactory2Trait, IPluginFactoryTrait, PClassInfo, PClassInfo2,
};

/// Load a VST3 dynamic library to extract its plugin descriptors.
pub fn probe_vst3(library_path: &Path) -> Result<Vec<PluginDescriptor>, String> {
    let mut descriptors = Vec::new();

    let lib = Vst3Library::new(library_path)?;
    let factory_ptr = lib.get_factory()?;

    // Need factory2 for subcategory info
    let factory = unsafe { ComRef::<IPluginFactory2>::from_raw(factory_ptr as *mut _).unwrap() };

    let class_count = unsafe { factory.countClasses() };

    for i in 0..class_count {
        let mut class_info: PClassInfo = unsafe { std::mem::zeroed() };
        let res = unsafe { factory.getClassInfo(i, &mut class_info) };
        if res == kResultOk {
            let category = extract_cstring(&class_info.category);

            if category == "Audio Module Class" {
                let name = extract_cstring(&class_info.name);
                let mut is_instrument = false;
                let mut sub_category = category.clone();

                let mut class_info2: PClassInfo2 = unsafe { std::mem::zeroed() };
                let res = unsafe { factory.getClassInfo2(i, &mut class_info2) };
                if res == kResultOk {
                    sub_category = extract_cstring(&class_info2.subCategories);
                    is_instrument = sub_category.contains("Instrument");
                }

                descriptors.push(PluginDescriptor {
                    name,
                    category: sub_category,
                    format: PluginFormat::Vst3,
                    is_instrument,
                    library_path: library_path.to_path_buf(),
                });
            }
        }
    }

    Ok(descriptors)
}

/// Returns the standard system paths for VST3 depending on the OS
pub fn standard_vst3_paths() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    return vec![PathBuf::from(r"C:\Program Files\Common Files\VST3")];

    #[cfg(target_os = "macos")]
    return vec![
        PathBuf::from("/Library/Audio/Plug-Ins/VST3"),
        PathBuf::from(std::env::var("HOME").unwrap_or_default() + "/Library/Audio/Plug-Ins/VST3"),
    ];

    #[cfg(target_os = "linux")]
    return vec![
        PathBuf::from(std::env::var("HOME").unwrap_or_default() + "/.vst3"),
        PathBuf::from("/usr/lib/vst3"),
        PathBuf::from("/usr/local/lib/vst3"),
    ];
}

/// Recursively scan a folder for all supported plugin formats
pub fn scan_folder(dir: &Path) -> Vec<PluginDescriptor> {
    let mut results = Vec::new();

    if !dir.exists() {
        return results;
    }

    scan_recursive(dir, &mut results);
    results
}

fn scan_recursive(dir: &Path, results: &mut Vec<PluginDescriptor>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

        match ext {
            "vst3" => {
                // Resolve to the actual library
                if let Some(lib_path) = resolve_vst3_library(&path) {
                    match probe_vst3(&lib_path) {
                        Ok(descriptors) => results.extend(descriptors),
                        Err(e) => eprintln!("Failed to probe {:?}: {}", lib_path, e),
                    }
                }
            }
            _ => {
                // If it's a normal directory, recurse into it
                if path.is_dir() {
                    scan_recursive(&path, results);
                }
            }
        }
    }
}

/// Given a path ending in .vst3 (file or directory), returns the path to the actual dynamic library.
fn resolve_vst3_library(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }

    if path.is_dir() {
        // The inner executable usually shares the same name as the folder's stem
        let file_stem = path.file_stem()?;

        #[cfg(target_os = "windows")]
        let inner_path = path
            .join("Contents")
            .join("x86_64-win")
            .join(file_stem)
            .with_extension("vst3");

        // TODO: test this
        #[cfg(target_os = "macos")]
        let inner_path = path.join("Contents").join("MacOS").join(file_stem);

        // TODO: test this
        #[cfg(target_os = "linux")]
        let inner_path = path
            .join("Contents")
            .join("x86_64-linux")
            .join(file_stem)
            .with_extension("so");

        if inner_path.is_file() {
            return Some(inner_path);
        }
    }

    None
}
