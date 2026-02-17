---
paths: '**/Services/**/*.swift,**/Whisper/**/*.swift'
---

# Thoth Service Architecture

Service design patterns and conventions for Thoth.

## Protocol-Based Services

- Must use protocols for services with multiple implementations (e.g., transcription providers)
- Must define protocol with `func ... async throws` signatures
- Must create concrete implementations conforming to the protocol

## Service Registry Pattern

- Must use a registry class to manage multiple service implementations
- Must use `private(set) lazy var` for expensive service instantiation
- Must provide `service(for: Provider)` method for provider selection
- Must mark registry with `@MainActor` when coordinating UI state

## Singleton Services

- Must use `static let shared` pattern for stateless utility services
- Must use `private init()` to prevent external instantiation

**Appropriate for singletons:**

- `KeychainService` - Keychain access
- `CustomVocabularyService` - Dictionary lookups
- `WordReplacementService` - Text processing
- `AudioDeviceManager` - System audio devices
- `SoundManager` - Audio playback
- `NotificationManager` - System notifications

**Not appropriate for singletons:**

- Services requiring `ModelContext` (use dependency injection)
- Services with complex state (use instance per context)

## Dependency Injection

- Must use constructor injection for services needing `ModelContext` or complex dependencies
- Must provide default values for optional dependencies: `init(service: Service = Service())`
- Must use lazy initialisation for expensive dependencies

## Naming Conventions

| Type           | Pattern                  | Example                         |
| -------------- | ------------------------ | ------------------------------- |
| Protocol       | `*Service`               | `TranscriptionService`          |
| Implementation | `*Service` or `*Manager` | `LocalTranscriptionService`     |
| Registry       | `*Registry`              | `TranscriptionServiceRegistry`  |
| Coordinator    | `*Coordinator`           | `WhisperModelWarmupCoordinator` |

## Logger Pattern

- Must include a logger in every service
- Must set category to match class name: `category: "MyService"`
- Must use subsystem `"com.poodle64.thoth"`

## Error Handling

- Must define service-specific error enums conforming to `Error, LocalizedError, Identifiable`
- Must provide `errorDescription` for user-facing messages
- Must provide `recoverySuggestion` where applicable
- Must NOT swallow errors - let caller handle them

## Actor Isolation

- Must use `@MainActor` for services with `@Published` properties
- Must use `@MainActor` for services coordinating UI state
- Must NOT use `@MainActor` for pure utility services (use thread-safe APIs or `NSLock`)

## Key Directives

- **Use protocols** for services with multiple implementations
- **Use registry pattern** to manage provider selection
- **Use singletons** only for stateless utility services
- **Use constructor injection** for services needing `ModelContext`
- **Every service has a logger** with category matching class name

## See Also

- [docs/architecture/service-patterns.md](../../docs/architecture/service-patterns.md) - Detailed patterns and examples
- [docs/architecture/overview.md](../../docs/architecture/overview.md) - High-level architecture
