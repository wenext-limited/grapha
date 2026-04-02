use super::IndexStoreStatus;
#[cfg(not(no_swift_bridge))]
use std::fs;
#[cfg(not(no_swift_bridge))]
use tempfile::tempdir;

fn assert_indexstore_open_signature(_: super::IndexStoreOpenFn) {}
fn assert_indexstore_close_signature(_: super::IndexStoreCloseFn) {}
fn assert_indexstore_extract_signature(_: super::IndexStoreExtractFn) {}

unsafe extern "C" fn sample_indexstore_open(_: *const i8, _: *mut i32) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}

unsafe extern "C" fn sample_indexstore_close(_: *mut std::ffi::c_void) {}

unsafe extern "C" fn sample_indexstore_extract(
    _: *mut std::ffi::c_void,
    _: *const i8,
    _: *mut u32,
    _: *mut i32,
) -> *const u8 {
    std::ptr::null()
}

#[test]
fn ffi_aliases_use_explicit_status_signatures() {
    assert_indexstore_open_signature(sample_indexstore_open);
    assert_indexstore_close_signature(sample_indexstore_close);
    assert_indexstore_extract_signature(sample_indexstore_extract);
}

#[test]
fn decodes_known_status_codes() {
    assert_eq!(IndexStoreStatus::try_from(0).unwrap(), IndexStoreStatus::Ok);
    assert_eq!(
        IndexStoreStatus::try_from(1).unwrap(),
        IndexStoreStatus::OpenFailed
    );
    assert_eq!(
        IndexStoreStatus::try_from(2).unwrap(),
        IndexStoreStatus::InvalidHandle
    );
    assert_eq!(
        IndexStoreStatus::try_from(3).unwrap(),
        IndexStoreStatus::ExtractFailed
    );
}

#[test]
fn rejects_unknown_status_codes() {
    assert!(IndexStoreStatus::try_from(99).is_err());
}

#[cfg(not(no_swift_bridge))]
#[test]
fn resolves_packaged_dylib_next_to_executable() {
    let bundle_dir = tempdir().unwrap();
    let executable = bundle_dir.path().join("grapha");
    let dylib = bundle_dir.path().join(super::BRIDGE_DYLIB_NAME);
    fs::write(&executable, b"binary").unwrap();
    fs::write(&dylib, b"dylib").unwrap();

    let resolved = super::resolve_existing_dylib(None, Some(&executable), None);

    assert_eq!(resolved, Some(dylib));
}

#[cfg(not(no_swift_bridge))]
#[test]
fn runtime_override_takes_precedence_over_packaged_location() {
    let override_dir = tempdir().unwrap();
    let bundle_dir = tempdir().unwrap();
    let build_dir = tempdir().unwrap();

    let executable = bundle_dir.path().join("grapha");
    let override_dylib = override_dir.path().join(super::BRIDGE_DYLIB_NAME);
    let packaged_dylib = bundle_dir.path().join(super::BRIDGE_DYLIB_NAME);
    let build_dylib = build_dir.path().join(super::BRIDGE_DYLIB_NAME);

    fs::write(&executable, b"binary").unwrap();
    fs::write(&override_dylib, b"dylib").unwrap();
    fs::write(&packaged_dylib, b"dylib").unwrap();
    fs::write(&build_dylib, b"dylib").unwrap();

    let resolved = super::resolve_existing_dylib(
        Some(override_dir.path()),
        Some(&executable),
        Some(build_dir.path()),
    );

    assert_eq!(resolved, Some(override_dylib));
}

#[cfg(not(no_swift_bridge))]
#[test]
fn build_output_is_last_fallback_candidate() {
    let build_dir = tempdir().unwrap();
    let build_dylib = build_dir.path().join(super::BRIDGE_DYLIB_NAME);
    fs::write(&build_dylib, b"dylib").unwrap();

    let resolved = super::resolve_existing_dylib(None, None, Some(build_dir.path()));

    assert_eq!(resolved, Some(build_dylib));
}
