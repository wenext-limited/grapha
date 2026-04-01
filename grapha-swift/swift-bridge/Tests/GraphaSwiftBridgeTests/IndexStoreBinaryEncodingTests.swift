import XCTest
@testable import GraphaSwiftBridge

final class IndexStoreBinaryEncodingTests: XCTestCase {
    private func readU32(_ bytes: [UInt8], at offset: Int) -> UInt32 {
        UInt32(bytes[offset])
            | (UInt32(bytes[offset + 1]) << 8)
            | (UInt32(bytes[offset + 2]) << 16)
            | (UInt32(bytes[offset + 3]) << 24)
    }

    func testBinaryFixtureIncludesImportRecordsAndEndPositions() {
        let (buffer, length) = makeBinaryFixtureForTests()
        defer { free(buffer) }

        let bytes = Array(UnsafeRawBufferPointer(start: buffer, count: Int(length)))
        let stringTableOffset = Int(readU32(bytes, at: 20))
        let importCount = Int(readU32(bytes, at: 16))
        let startLine = readU32(bytes, at: 56)
        let startCol = readU32(bytes, at: 60)
        let endLine = readU32(bytes, at: 64)
        let endCol = readU32(bytes, at: 68)
        let importPathOffset = Int(readU32(bytes, at: 76))
        let importPathLength = Int(readU32(bytes, at: 80))
        let importKind = bytes[84]
        let importStart = stringTableOffset + importPathOffset
        let importEnd = importStart + importPathLength
        let importPath = String(decoding: bytes[importStart..<importEnd], as: UTF8.self)

        XCTAssertEqual(bytes[4], 2)
        XCTAssertGreaterThan(importCount, 0)
        XCTAssertEqual(importKind, 2)
        XCTAssertEqual(importPath, "Foundation")
        XCTAssertEqual(startLine, 4)
        XCTAssertEqual(startCol, 2)
        XCTAssertEqual(endLine, 4)
        XCTAssertEqual(endCol, 15)
        XCTAssertNotEqual(endCol, startCol)
    }

    func testLiveIndexStoreFallbackUsesPointSpanWithoutExactRangeData() {
        let end = resolvedEndPositionForTests(startLine: 7, startCol: 3, exactEnd: nil)
        XCTAssertEqual(end.0, 7)
        XCTAssertEqual(end.1, 3)
    }
}
