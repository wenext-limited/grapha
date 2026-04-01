import XCTest
@testable import GraphaSwiftBridge

final class BridgeExportsTests: XCTestCase {
    private func makeTemporaryDirectory() throws -> String {
        let path = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: path, withIntermediateDirectories: true)
        return path.path
    }

    func testIndexStoreOpenReportsOpenFailureStatus() {
        var status: Int32 = -1
        let handle = "/tmp/missing-index-store".withCString { path in
            indexstoreOpen(path, &status)
        }

        XCTAssertNil(handle)
        XCTAssertEqual(status, 1)
    }

    func testIndexStoreExtractRejectsInvalidHandle() {
        var length: UInt32 = 123
        var status: Int32 = -1
        let buffer = "File.swift".withCString { filePath in
            indexstoreExtract(nil, filePath, &length, &status)
        }

        XCTAssertNil(buffer)
        XCTAssertEqual(length, 0)
        XCTAssertEqual(status, 2)
    }

    func testIndexStoreCloseAcceptsNilHandle() {
        indexstoreClose(nil)
    }

    func testIndexStoreCloseInvalidatesHandle() throws {
        let storePath = try makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(atPath: storePath) }

        var openStatus: Int32 = -1
        let handle = storePath.withCString { path in
            indexstoreOpen(path, &openStatus)
        }

        XCTAssertNotNil(handle)
        XCTAssertEqual(openStatus, 0)

        indexstoreClose(handle)

        var length: UInt32 = 123
        var extractStatus: Int32 = -1
        let buffer = "File.swift".withCString { filePath in
            indexstoreExtract(handle, filePath, &length, &extractStatus)
        }

        XCTAssertNil(buffer)
        XCTAssertEqual(length, 0)
        XCTAssertEqual(extractStatus, 2)
    }
}
