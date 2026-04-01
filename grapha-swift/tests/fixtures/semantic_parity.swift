import SwiftUI

protocol Runnable {}
class Base {}
class Worker: Base, Runnable {}

enum L10n {
    static var greeting: String {
        L10n.tr("Localizable", "greeting", fallback: "Hello")
    }

    static func tr(_ table: String, _ key: String, fallback: String) -> String {
        fallback
    }
}

struct ContentView: View {
    @State private var count = 0

    var doubled: Int {
        count * 2
    }

    var badge: some View {
        Image("feature_badge")
    }

    var body: some View {
        VStack {
            Text(L10n.greeting)
            badge
            Text("\(doubled)")
        }
    }
}
