import AppKit
import CryptoKit
import Foundation
import LocalAuthentication

func b64url(_ d: Data) -> String {
    d.base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
}

func fail(_ m: String, _ code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data((m + "\n").utf8))
    exit(code)
}

func pae(_ type: String, _ payload: Data) -> Data {
    var out = Data("DSSEv1 \(type.utf8.count) \(type) \(payload.count) ".utf8)
    out.append(payload)
    return out
}

func arg(_ name: String) -> String? {
    let a = CommandLine.arguments
    guard let i = a.firstIndex(of: name), i + 1 < a.count else { return nil }
    return a[i + 1]
}

func matches(_ s: String, _ pattern: String) -> [String]? {
    guard let re = try? NSRegularExpression(pattern: pattern),
          let m = re.firstMatch(in: s, range: NSRange(s.startIndex..., in: s)) else { return nil }
    return (1..<m.numberOfRanges).map { String(s[Range(m.range(at: $0), in: s)!]) }
}

func load(_ path: String, _ ctx: LAContext?) -> SecureEnclave.P256.Signing.PrivateKey {
    guard let blob = FileManager.default.contents(atPath: path) else { fail("no key at \(path)") }
    do { return try SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: blob, authenticationContext: ctx) }
    catch { fail("could not load the key (it belongs to this Mac's Secure Enclave): \(error)") }
}

func envelope(_ type: String, _ payload: Data, _ k: SecureEnclave.P256.Signing.PrivateKey) throws -> String {
    let sig = try k.signature(for: pae(type, payload))
    return "{\"alg\":\"p256-se\",\"payload\":\"\(b64url(payload))\",\"public\":\"\(b64url(k.publicKey.x963Representation))\",\"sig\":\"\(b64url(sig.rawRepresentation))\",\"type\":\"\(type)\"}"
}

// One approval on screen at a time, across processes.
func takeLock(_ dir: String) {
    let fd = open(dir + "/approver.lock", O_CREAT | O_RDWR, 0o600)
    if fd < 0 || flock(fd, LOCK_EX | LOCK_NB) != 0 { fail("another approval is already on screen", 6) }
}

#if KAKU_TEST_AUTOCONFIRM
// Test build only. The shipped helper has no way to skip the dialog.
func confirm(title: String, body: String, approve: String) -> Bool {
    FileHandle.standardError.write(Data(("SHOWN: " + title + "\n" + body + "\n").utf8))
    if let ms = ProcessInfo.processInfo.environment["KAKU_TEST_HOLD_MS"], let n = UInt32(ms) { usleep(n * 1000) }
    return ProcessInfo.processInfo.environment["KAKU_TEST_DECISION"] != "refuse"
}
#else
func confirm(title: String, body: String, approve: String) -> Bool {
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    app.activate(ignoringOtherApps: true)
    let alert = NSAlert()
    alert.alertStyle = .critical
    alert.messageText = title
    alert.informativeText = body
    alert.addButton(withTitle: "Refuse")
    alert.addButton(withTitle: approve)
    alert.buttons[0].keyEquivalent = "\r"
    alert.buttons[1].keyEquivalent = ""
    return alert.runModal() == .alertSecondButtonReturn
}
#endif

func fetchApproval(dataDir: String, id: String) -> [String: Any] {
    guard let raw = FileManager.default.contents(atPath: dataDir + "/admin.json"),
          let admin = try? JSONSerialization.jsonObject(with: raw) as? [String: Any],
          let port = admin["port"] as? Int, let token = admin["token"] as? String,
          let url = URL(string: "http://127.0.0.1:\(port)/admin/approvals/\(id)") else { fail("could not read the kernel's admin file in \(dataDir)") }
    var req = URLRequest(url: url, timeoutInterval: 15)
    req.setValue("Bearer " + token, forHTTPHeaderField: "Authorization")
    let sem = DispatchSemaphore(value: 0)
    var result: (Data?, Int) = (nil, 0)
    URLSession.shared.dataTask(with: req) { d, r, _ in result = (d, (r as? HTTPURLResponse)?.statusCode ?? 0); sem.signal() }.resume()
    _ = sem.wait(timeout: .now() + 20)
    guard result.1 == 200, let d = result.0, let j = try? JSONSerialization.jsonObject(with: d) as? [String: Any] else { fail("the kernel does not know this approval (HTTP \(result.1))", 7) }
    return j
}

guard SecureEnclave.isAvailable else { fail("this Mac has no Secure Enclave", 3) }
let cmd = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : ""
let usage = "usage: kaku-enclave enroll --blob P [--presence] | public --blob P | approve --blob P --data-dir D --approval ID | change --blob P --data-dir D --action enroll|revoke --subject KEYID"
guard let path = arg("--blob") else { fail(usage, 2) }

switch cmd {
case "enroll":
    if FileManager.default.fileExists(atPath: path) { fail("a key already exists at \(path); it is never overwritten") }
    var flags: SecAccessControlCreateFlags = [.privateKeyUsage]
    if CommandLine.arguments.contains("--presence") { flags.insert(.userPresence) }
    guard let ac = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, flags, nil) else { fail("access control failed") }
    do {
        let k = try SecureEnclave.P256.Signing.PrivateKey(accessControl: ac)
        guard FileManager.default.createFile(atPath: path, contents: k.dataRepresentation, attributes: [.posixPermissions: 0o600]) else { fail("could not write \(path)") }
        print(b64url(k.publicKey.x963Representation))
    } catch { fail("could not create the key: \(error)") }
case "public":
    print(b64url(load(path, nil).publicKey.x963Representation))
case "approve":
    guard let dir = arg("--data-dir"), let id = arg("--approval"), matches(id, "^([A-Za-z0-9_-]{1,64})$") != nil else { fail(usage, 2) }
    takeLock(dir)
    let detail = fetchApproval(dataDir: dir, id: id)
    guard let st = detail["statement_canonical"] as? String,
          let f = matches(st, "^\\{\"approval\":\"([A-Za-z0-9_-]{1,64})\",\"digest\":\"([0-9a-f]{64})\",\"expires\":([0-9]{1,12}),\"job\":\"([A-Za-z0-9_-]{1,64})\",\"nonce\":\"([A-Za-z0-9_-]{8,64})\",\"policy\":\"([A-Za-z0-9_.-]{1,64})\",\"recipient\":\"([ !#-\\[\\]-~]{1,80})\",\"v\":1\\}$") else {
        fail("the kernel's approval statement is not in the exact form this helper signs (recipients must be printable ASCII); refusing", 8)
    }
    guard f[0] == id else { fail("the statement is for a different approval; refusing", 8) }
    guard (detail["digest"] as? String) == f[1], (detail["recipient"] as? String) == f[6] else { fail("the approval details do not match the statement; refusing", 8) }
    guard let expires = Int(f[2]), expires > Int(Date().timeIntervalSince1970) else { fail("this approval has expired; refusing", 9) }
    var lines = ["Recipient: \(f[6])", "Release fingerprint (sha-256): \(f[1])", "Policy: \(f[5])", "Approval: \(f[0])   Job: \(f[3])"]
    if let files = detail["files"] as? [[String: Any]] {
        lines.append("")
        lines.append("Exactly these \(files.count) file(s) will be handed over:")
        for x in files.prefix(40) { lines.append("  \(x["name"] as? String ?? "?")  (\(x["bytes"] as? Int ?? 0) bytes)") }
        if files.count > 40 { lines.append("  ... and \(files.count - 40) more") }
    }
    if let removed = detail["removed"] as? [[String: Any]], !removed.isEmpty { lines.append(""); lines.append("Withheld: \(removed.count) item(s)") }
    if let d = detail["disclosure"] as? [String: Any], let after = d["files_after"], let total = d["project_files"] { lines.append("Cumulative: \(after) of \(total) project files would have been shown to this recipient") }
    lines.append("")
    lines.append("Your Mac will then ask for Touch ID or your account password. Approving is final for this release.")
    guard confirm(title: "Approve this release to \(f[6])?", body: lines.joined(separator: "\n"), approve: "Approve...") else { fail("cancelled: nothing was signed", 5) }
    let ctx = LAContext()
    ctx.localizedReason = "Approve release \(f[1].prefix(12)) to \(f[6])"
    do { print(try envelope("application/vnd.kakurega.approval+json", Data(st.utf8), load(path, ctx))) }
    catch { fail("signing was refused or failed: \(error)", 4) }
case "change":
    guard let dir = arg("--data-dir"), let action = arg("--action"), ["enroll", "revoke"].contains(action),
          let subject = arg("--subject"), matches(subject, "^((?:ed25519|p256):[0-9a-f]{64})$") != nil else { fail(usage, 2) }
    takeLock(dir)
    let at = Int(Date().timeIntervalSince1970)
    let payload = "{\"action\":\"\(action)\",\"at\":\(at),\"subject\":\"\(subject)\",\"v\":1}"
    let verb = action == "enroll" ? "ADD this key as an approver" : "REMOVE this approver key"
    guard confirm(title: "\(verb)?", body: "Key: \(subject)\n\nAnyone holding an approver key can approve releases of your files. Approve only if you asked for this.", approve: "Continue...") else { fail("cancelled: nothing was signed", 5) }
    let ctx = LAContext()
    ctx.localizedReason = "\(verb) for Kakurega"
    do { print(try envelope("application/vnd.kakurega.approver-action+json", Data(payload.utf8), load(path, ctx))) }
    catch { fail("signing was refused or failed: \(error)", 4) }
default:
    fail(usage, 2)
}
