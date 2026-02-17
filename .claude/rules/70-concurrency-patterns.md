---
paths: '**/*.swift'
---

# Thoth Concurrency Patterns

Async/await and actor isolation patterns for Thoth.

## State Machine Pattern

- Must use explicit state enum for recording lifecycle (`idle`, `recording`, `transcribing`, `enhancing`)
- Must guard state transitions to prevent invalid states
- Must use `defer` to reset state on completion/error

## @MainActor Usage

**Must use `@MainActor` for:**

- Classes with `@Published` properties
- Classes coordinating UI state (e.g., `WhisperState`, `TranscriptionServiceRegistry`)
- View models and state containers

**Must NOT use `@MainActor` for:**

- Pure utility/stateless services (e.g., `KeychainService`)
- Background processing classes
- Core Audio classes (use `NSLock` instead)

## Dispatching to Main Actor

- Must use `Task { @MainActor in }` for UI updates from background work
- Must use `await MainActor.run { }` for synchronous main-thread blocks
- Must use `await` when accessing `@MainActor` properties from non-isolated contexts

## Task Lifecycle

- Must use `.task()` modifier for view-owned async work (auto-cancelled)
- Must use `.task(id:)` when work should restart on value change
- Must cancel tasks in `deinit` to prevent leaks
- Must store long-lived tasks as `Task<T, Error>?` properties

## Async/Await Patterns

- Must use `async throws` for all I/O operations
- Must propagate errors - NEVER swallow them in async functions
- Must use `withThrowingTaskGroup` for timeout patterns

## Structured Concurrency

- Must use `TaskGroup` for parallel independent operations
- Must use sequential `await` for dependent operations
- Must call `group.cancelAll()` after getting first result in race patterns

## Published Property Updates

- Must use `Task { }` for async work triggered by `didSet`
- Must NOT perform long synchronous operations in `didSet`

## Thread Safety Summary

| Context             | Approach                                     |
| ------------------- | -------------------------------------------- |
| UI state classes    | `@MainActor`                                 |
| Service registries  | `@MainActor`                                 |
| Audio callbacks     | `NSLock`                                     |
| Singleton utilities | No isolation (stateless or thread-safe APIs) |
| Background work     | Async tasks, no actor                        |

## Key Directives

- **Use `@MainActor`** on classes with `@Published` properties
- **Use state enums** for recording lifecycle management
- **Use `.task()` modifier** for view-owned async work
- **Propagate errors** - don't swallow them in async functions
- **Use `NSLock`** for thread-safe access in audio callbacks (not actors)
- **Cancel tasks** in `deinit` to prevent leaks

## See Also

- [docs/architecture/concurrency-patterns.md](../../docs/architecture/concurrency-patterns.md) - Detailed patterns and examples
