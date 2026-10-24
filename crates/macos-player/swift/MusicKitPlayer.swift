import Foundation
import MusicKit

private let player = ApplicationMusicPlayer.shared
private let deadline: TimeInterval = 15
private let consentDeadline: TimeInterval = 300

private struct Failure: Encodable, Error {
    struct Detail: Encodable {
        var type: String
        var message: String
    }
    var error: Detail
    init(_ type: String, _ message: String) { error = Detail(type: type, message: message) }
    init(_ cause: Error) { self.init(String(describing: Swift.type(of: cause)), String(describing: cause)) }
}

private struct Row: Encodable {
    var id: String
    var title: String?
    var duration: Double?
}

private struct Snap: Encodable {
    var state: String
    var position: Double?
    var duration: Double?
    var itemId: String?
    var title: String?
    var artist: String?
    var queue: [Row]
    var queuePosition: Int?
    var queueLength: Int
    var shuffle: Bool
    var repeatMode: String

    enum CodingKeys: String, CodingKey {
        case state, position, duration, itemId, title, artist, queue, queuePosition, queueLength, shuffle
        case repeatMode = "repeat"
    }
}

private final class Slot<T: Sendable>: @unchecked Sendable {
    let done = DispatchSemaphore(value: 0)
    var result: Result<T, Error>?
}

private func wait(_ done: DispatchSemaphore, _ seconds: TimeInterval) -> Bool {
    guard Thread.isMainThread else { return done.wait(timeout: .now() + seconds) == .success }
    let end = Date(timeIntervalSinceNow: seconds)
    while Date() < end {
        if done.wait(timeout: .now()) == .success { return true }
        RunLoop.main.run(mode: .default, before: Date(timeIntervalSinceNow: 0.02))
    }
    return false
}

private func blocking<T: Sendable>(_ seconds: TimeInterval = deadline, _ work: @escaping @Sendable () async throws -> T) throws -> T {
    let slot = Slot<T>()
    Task {
        do { slot.result = .success(try await work()) } catch { slot.result = .failure(error) }
        slot.done.signal()
    }
    guard wait(slot.done, seconds), let result = slot.result else {
        throw Failure("Timeout", "MusicKit did not answer within \(Int(seconds)) seconds")
    }
    return try result.get()
}

private func reply<T: Encodable>(_ value: T) -> UnsafeMutablePointer<CChar> {
    let data = (try? JSONEncoder().encode(value)) ?? Data(#"{"error":{"type":"Encode","message":"reply"}}"#.utf8)
    return strdup(String(decoding: data, as: UTF8.self))
}

private func library(_ batch: [String: Any]) -> Bool { batch["library"] as? Bool ?? false }

private func songs(_ batch: [String: Any]) async throws -> [Song] {
    let ids = (batch["ids"] as? [String] ?? []).map { MusicItemID($0) }
    guard library(batch) else {
        return Array(try await MusicCatalogResourceRequest<Song>(matching: \.id, memberOf: ids).response().items)
    }
    var request = MusicLibraryRequest<Song>()
    request.filter(matching: \.id, memberOf: ids)
    request.limit = ids.count
    return Array(try await request.response().items)
}

private func every(_ first: MusicItemCollection<Track>?) async throws -> [Track] {
    guard var page = first else { return [] }
    var all = Array(page)
    while page.hasNextBatch, let next = try await page.nextBatch() {
        all += next
        page = next
    }
    return all
}

private func album(_ id: MusicItemID, _ inLibrary: Bool) async throws -> [Track] {
    if inLibrary {
        var request = MusicLibraryRequest<Album>()
        request.filter(matching: \.id, equalTo: id)
        return try await every(try await request.response().items.first?.with([.tracks]).tracks)
    }
    let found = try await MusicCatalogResourceRequest<Album>(matching: \.id, equalTo: id).response().items.first
    return try await every(try await found?.with([.tracks]).tracks)
}

private func playlist(_ id: MusicItemID, _ inLibrary: Bool) async throws -> [Track] {
    if inLibrary {
        var request = MusicLibraryRequest<Playlist>()
        request.filter(matching: \.id, equalTo: id)
        return try await every(try await request.response().items.first?.with([.tracks]).tracks)
    }
    let found = try await MusicCatalogResourceRequest<Playlist>(matching: \.id, equalTo: id).response().items.first
    return try await every(try await found?.with([.tracks]).tracks)
}

private func resolve(_ c: [String: Any]) async throws -> [Track] {
    let id = MusicItemID(c["id"] as? String ?? "")
    switch c["kind"] as? String ?? "" {
    case "songs":
        var found: [MusicItemID: Track] = [:]
        var failure: Error?
        for batch in c["batches"] as? [[String: Any]] ?? [] {
            do { for song in try await songs(batch) { found[song.id] = .song(song) } } catch { failure = failure ?? error }
        }
        let resolved = (c["ids"] as? [String] ?? []).compactMap { found[MusicItemID($0)] }
        if resolved.isEmpty, let failure { throw failure }
        return resolved
    case "album": return try await album(id, library(c))
    case "playlist": return try await playlist(id, library(c))
    default: throw Failure("Command", "unknown item kind")
    }
}

private func items(_ c: [String: Any]) throws -> [Track] {
    try blocking { try await resolve(c) }
}

private func startIndex(_ c: [String: Any], _ tracks: [Track]) -> Int {
    let wanted = c["start"] as? Int ?? 0
    guard c["kind"] as? String == "songs", let ids = c["ids"] as? [String], ids.indices.contains(wanted) else {
        return min(wanted, tracks.count - 1)
    }
    let position = Dictionary(tracks.enumerated().map { ($1.id.rawValue, $0) }, uniquingKeysWith: { first, _ in first })
    return ids[wanted...].compactMap { position[$0] }.first ?? 0
}

private func perform(_ name: String, _ c: [String: Any]) throws {
    switch name {
    case "setQueue":
        let tracks = try items(c)
        guard !tracks.isEmpty else {
            let asked = (c["ids"] as? [String]).map { " (resolved 0 of \($0.count) asked)" } ?? ""
            throw Failure("Command", "no playable items for the queue\(asked)")
        }
        player.queue = ApplicationMusicPlayer.Queue(for: tracks, startingAt: tracks[startIndex(c, tracks)])
    case "play": try blocking { try await player.play() }
    case "pause": player.pause()
    case "seek": player.playbackTime = c["seconds"] as? Double ?? 0
    case "next": try blocking { try await player.skipToNextEntry() }
    case "previous": try blocking { try await player.skipToPreviousEntry() }
    case "shuffle": player.state.shuffleMode = (c["on"] as? Bool ?? false) ? .songs : .off
    case "repeat":
        let modes: [String: MusicPlayer.RepeatMode] = ["off": .none, "one": .one, "all": .all]
        player.state.repeatMode = modes[c["mode"] as? String ?? ""] ?? MusicPlayer.RepeatMode.none
    case "append", "playNext":
        let tracks = try items(c), position: MusicPlayer.Queue.EntryInsertionPosition = name == "append" ? .tail : .afterCurrentEntry
        try blocking { try await player.queue.insert(tracks, position: position) }
    case "jump":
        let index = c["index"] as? Int ?? -1
        let entries = player.queue.entries
        guard entries.indices.contains(index), songID(entries[index]) == c["id"] as? String else {
            throw Failure("Command", "the queue changed before the jump to index \(index)")
        }
        player.queue.currentEntry = entries[index]
        try blocking { try await player.play() }
    default: throw Failure("Command", "unknown command \(name)")
    }
}

private func songID(_ entry: MusicPlayer.Queue.Entry) -> String {
    if case .song(let song)? = entry.item { return song.id.rawValue }
    return entry.id
}

private func songDuration(_ entry: MusicPlayer.Queue.Entry) -> Double? {
    if case .song(let song)? = entry.item { return song.duration }
    return nil
}

@_cdecl("bramley_mk_free")
public func bramleyFree(_ reply: UnsafeMutablePointer<CChar>?) { free(reply) }

@_cdecl("bramley_mk_authorization")
public func bramleyAuthorization(_ request: Bool) -> UnsafeMutablePointer<CChar> {
    do {
        let status = request ? try blocking(consentDeadline) { await MusicAuthorization.request() } : MusicAuthorization.currentStatus
        return reply(["status": status.rawValue])
    } catch { return reply(Failure(error)) }
}

@_cdecl("bramley_mk_tokens")
public func bramleyTokens(_ fresh: Bool) -> UnsafeMutablePointer<CChar> {
    do {
        let provider = DefaultMusicTokenProvider()
        let options: MusicTokenRequestOptions = fresh ? [.ignoreCache] : []
        let pair = try blocking {
            let developer = try await provider.developerToken(options: options)
            return [developer, try await provider.userToken(for: developer, options: options)]
        }
        return reply(["developer": pair[0], "user": pair[1]])
    } catch { return reply(Failure(error)) }
}

@_cdecl("bramley_mk_command")
public func bramleyCommand(_ json: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar> {
    guard let c = try? JSONSerialization.jsonObject(with: Data(String(cString: json).utf8)) as? [String: Any],
          let name = c["cmd"] as? String
    else { return reply(Failure("Command", "malformed command")) }
    do {
        try perform(name, c)
        return reply(["ok": true])
    } catch { return reply(Failure(error)) }
}

@_cdecl("bramley_mk_snapshot")
public func bramleySnapshot(_ shown: Int32) -> UnsafeMutablePointer<CChar> {
    let entries = Array(player.queue.entries)
    let current = player.queue.currentEntry
    let at = current.flatMap { c in entries.firstIndex { $0.id == c.id } }
    let state: String
    switch player.state.playbackStatus {
    case .playing, .seekingForward, .seekingBackward: state = "playing"
    case .paused: state = "paused"
    case .interrupted: state = "loading"
    default: state = "stopped"
    }
    let repeating: String
    switch player.state.repeatMode {
    case .one?: repeating = "one"
    case .all?: repeating = "all"
    default: repeating = "off"
    }
    return reply(Snap(
        state: state,
        position: current == nil ? nil : player.playbackTime,
        duration: current.flatMap(songDuration),
        itemId: current.map(songID),
        title: current?.title,
        artist: current?.subtitle,
        queue: entries.dropFirst(at ?? 0).prefix(Int(shown)).map { Row(id: songID($0), title: $0.title, duration: songDuration($0)) },
        queuePosition: at,
        queueLength: entries.count,
        shuffle: (player.state.shuffleMode ?? .off) != .off,
        repeatMode: repeating))
}
