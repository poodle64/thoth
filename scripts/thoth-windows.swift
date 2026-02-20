#!/usr/bin/env swift
// Lists Thoth windows as JSON for screenshot automation.
// Usage: swift scripts/thoth-windows.swift [--all]
//   --all  Include off-screen and zero-sized windows (default: main windows only)

import CoreGraphics
import Foundation

let showAll = CommandLine.arguments.contains("--all")

let windowList = CGWindowListCopyWindowInfo(
    [.optionAll],
    kCGNullWindowID
) as? [[String: Any]] ?? []

var results: [[String: Any]] = []

for window in windowList {
    guard let ownerName = window[kCGWindowOwnerName as String] as? String,
          ownerName.lowercased() == "thoth" else { continue }

    let windowID = window[kCGWindowNumber as String] as? Int ?? 0
    let windowName = window[kCGWindowName as String] as? String ?? ""
    let bounds = window[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let layer = window[kCGWindowLayer as String] as? Int ?? -1
    let isOnScreen = window[kCGWindowIsOnscreen as String] as? Bool ?? false

    let width = bounds["Width"] as? Int ?? 0
    let height = bounds["Height"] as? Int ?? 0

    // Skip tiny windows (menu bar icon, status items) and layer != 0 (overlays)
    if !showAll && (width < 100 || height < 100 || layer != 0) {
        continue
    }

    let entry: [String: Any] = [
        "id": windowID,
        "name": windowName,
        "width": width,
        "height": height,
        "layer": layer,
        "onScreen": isOnScreen,
    ]
    results.append(entry)
}

// Sort by name (named windows first, then by ID)
results.sort { a, b in
    let nameA = a["name"] as? String ?? ""
    let nameB = b["name"] as? String ?? ""
    if !nameA.isEmpty && nameB.isEmpty { return true }
    if nameA.isEmpty && !nameB.isEmpty { return false }
    return (a["id"] as? Int ?? 0) < (b["id"] as? Int ?? 0)
}

let json = try JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys])
print(String(data: json, encoding: .utf8) ?? "[]")
