use super::IndexStoreStatus;

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
