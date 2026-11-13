import EventKit
import Foundation

// kaku-calendar status | ask | events --days N | reminders
// One line of JSON on standard output. Calendar and reminders are read on this computer through the operating system's
// own EventKit; nothing is sent anywhere. `status` never asks for access, so it can never show a prompt; `ask` asks, and
// is run only after a person pressed a button that says so.

let maxItems = 200
let maxTitle = 200
let maxNotes = 500
let maxDays = 31

func answer(_ object: [String: Any]) -> Never {
    let data = (try? JSONSerialization.data(withJSONObject: object, options: [])) ?? Data("{\"error\":\"unreadable\"}".utf8)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
    exit(0)
}

func fail(_ code: String) -> Never {
    answer(["error": code])
}

func argument(_ name: String) -> String? {
    let a = CommandLine.arguments
    guard let i = a.firstIndex(of: name), i + 1 < a.count else { return nil }
    return a[i + 1]
}

/// Text from a calendar is data: control characters become spaces and it is cut to `limit` characters.
func clean(_ s: String?, _ limit: Int) -> String {
    guard let s = s else { return "" }
    let plain = String(s.unicodeScalars.map { CharacterSet.controlCharacters.contains($0) && $0 != "\n" ? " " : Character($0) })
    return plain.count > limit ? String(plain.prefix(limit)) + "…" : plain
}

func state(_ s: EKAuthorizationStatus) -> String {
    switch s {
    case .notDetermined: return "not_asked"
    case .restricted: return "restricted"
    case .denied: return "denied"
    case .fullAccess: return "allowed"
    case .writeOnly: return "write_only"
    @unknown default: return "unknown"
    }
}

func status() -> [String: Any] {
    [
        "calendar": state(EKEventStore.authorizationStatus(for: .event)),
        "reminders": state(EKEventStore.authorizationStatus(for: .reminder)),
    ]
}

let iso: ISO8601DateFormatter = {
    let f = ISO8601DateFormatter()
    f.formatOptions = [.withInternetDateTime]
    f.timeZone = TimeZone.current
    return f
}()

let store = EKEventStore()
let command = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ""

switch command {
case "status":
    answer(status())
case "ask":
    let done = DispatchSemaphore(value: 0)
    if #available(macOS 14.0, *) {
        store.requestFullAccessToEvents { _, _ in
            store.requestFullAccessToReminders { _, _ in done.signal() }
        }
    } else {
        store.requestAccess(to: .event) { _, _ in
            store.requestAccess(to: .reminder) { _, _ in done.signal() }
        }
    }
    // a person answers the prompt; after two minutes the answer so far is given
    _ = done.wait(timeout: .now() + 120)
    answer(status())
case "events":
    guard EKEventStore.authorizationStatus(for: .event) == .fullAccess else { fail("calendar_not_allowed") }
    guard let text = argument("--days"), let days = Int(text), days >= 1, days <= maxDays else { fail("bad_days") }
    let start = Calendar.current.startOfDay(for: Date())
    guard let end = Calendar.current.date(byAdding: .day, value: days, to: start) else { fail("bad_days") }
    let found = store.events(matching: store.predicateForEvents(withStart: start, end: end, calendars: nil))
        .sorted { $0.startDate < $1.startDate }
    let items: [[String: Any]] = found.prefix(maxItems).map { e in
        [
            "title": clean(e.title, maxTitle),
            "start": iso.string(from: e.startDate),
            "end": iso.string(from: e.endDate),
            "all_day": e.isAllDay,
            "location": clean(e.location, maxTitle),
            "calendar": clean(e.calendar?.title, maxTitle),
            "notes": clean(e.notes, maxNotes),
        ]
    }
    answer(["events": items, "from": iso.string(from: start), "to": iso.string(from: end), "truncated": found.count > maxItems])
case "reminders":
    guard EKEventStore.authorizationStatus(for: .reminder) == .fullAccess else { fail("reminders_not_allowed") }
    let done = DispatchSemaphore(value: 0)
    var found: [EKReminder] = []
    store.fetchReminders(matching: store.predicateForIncompleteReminders(withDueDateStarting: nil, ending: nil, calendars: nil)) { r in
        found = r ?? []
        done.signal()
    }
    guard done.wait(timeout: .now() + 30) == .success else { fail("timed_out") }
    let items: [[String: Any]] = found.prefix(maxItems).map { r in
        var due = ""
        if let c = r.dueDateComponents, let d = Calendar.current.date(from: c) { due = iso.string(from: d) }
        return [
            "title": clean(r.title, maxTitle),
            "due": due,
            "list": clean(r.calendar?.title, maxTitle),
            "notes": clean(r.notes, maxNotes),
        ]
    }
    answer(["reminders": items, "truncated": found.count > maxItems])
default:
    fail("usage: kaku-calendar status | ask | events --days N | reminders")
}
