mod build_support;

use std::path::Path;
use std::process::Command;

use build_support::{
    BRIDGE_INPUTS, BRIDGE_MODE_ENV, BridgeBuildResult, BridgeMode, PostBuildDecision,
    PreBuildDecision, parse_bridge_mode, post_build_decision, pre_build_decision,
};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(no_swift_bridge)");
    println!("cargo:rerun-if-env-changed={BRIDGE_MODE_ENV}");

    let mode = parse_bridge_mode(std::env::var(BRIDGE_MODE_ENV).ok().as_deref())
        .unwrap_or_else(|err| panic!("{err}"));

    for path in BRIDGE_INPUTS {
        println!("cargo:rerun-if-changed={path}");
    }

    if mode == BridgeMode::Off {
        disable_bridge("off (skipping Swift bridge build)");
        return;
    }

    let bridge_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("swift-bridge");
    let package_manifest = bridge_dir.join("Package.swift");
    match pre_build_decision(mode, package_manifest.exists()) {
        PreBuildDecision::Skip(message) => {
            disable_bridge(message);
            return;
        }
        PreBuildDecision::Build => {}
        PreBuildDecision::Panic(message) => panic!("{message}"),
    }

    let status = Command::new("swift")
        .args(["build", "-c", "release"])
        .current_dir(&bridge_dir)
        .status();

    let lib_path = bridge_dir.join(".build/release");
    let build_result = match status {
        Ok(s) if s.success() && lib_path.join("libGraphaSwiftBridge.dylib").exists() => {
            BridgeBuildResult::Success
        }
        Ok(s) if s.success() => BridgeBuildResult::MissingDylib,
        Ok(_) => BridgeBuildResult::FailedStatus,
        Err(_) => BridgeBuildResult::LaunchFailed,
    };

    match post_build_decision(mode, build_result) {
        PostBuildDecision::EnableBridge => {
            println!("cargo:warning=Swift bridge mode: {}", mode.as_str());
            println!("cargo:rustc-env=SWIFT_BRIDGE_PATH={}", lib_path.display());
        }
        PostBuildDecision::Disable(message) => {
            disable_bridge(message);
        }
        PostBuildDecision::Panic(message) => {
            panic!("{message}");
        }
    }
}

fn disable_bridge(message: &str) {
    println!("cargo:warning=Swift bridge mode: {message}");
    println!("cargo:rustc-cfg=no_swift_bridge");
}
