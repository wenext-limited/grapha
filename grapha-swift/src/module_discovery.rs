use std::path::Path;

use grapha_core::ModuleMap;

pub fn discover_swift_modules(root: &Path) -> ModuleMap {
    let mut modules = ModuleMap::new();
    discover_swift_packages_recursive(root, &mut modules);
    modules
}

fn discover_swift_packages_recursive(dir: &Path, modules: &mut ModuleMap) {
    let package_swift = dir.join("Package.swift");
    if package_swift.is_file() {
        let module_name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        let sources_dir = dir.join("Sources");
        let source_dir = if sources_dir.is_dir() {
            sources_dir
        } else {
            dir.to_path_buf()
        };
        modules
            .modules
            .entry(module_name)
            .or_default()
            .push(source_dir);
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || name == "node_modules"
            || name == "build"
            || name == "DerivedData"
            || name == "Pods"
        {
            continue;
        }
        discover_swift_packages_recursive(&path, modules);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_swift_package() {
        let dir = tempfile::tempdir().unwrap();
        let pkg_dir = dir.path().join("MyPackage");
        let sources_dir = pkg_dir.join("Sources");
        fs::create_dir_all(&sources_dir).unwrap();
        fs::write(pkg_dir.join("Package.swift"), "// swift-tools-version:5.5").unwrap();

        let modules = discover_swift_modules(dir.path());
        assert!(modules.modules.contains_key("MyPackage"));
    }
}
