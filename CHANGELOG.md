# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial project scaffold with monorepo structure
- Rust services: API gateway, Ingest service, Processor
- Python SDK (`mlrunx`) with async-first design
- Python integrations (`mlrunx-integrations`) for Lightning, HuggingFace, Optuna, Hydra
- Next.js UI skeleton with TypeScript and Tailwind v4
- gRPC protocol definitions (common, ingest, query protos)
- Docker Compose infrastructure (ClickHouse, PostgreSQL, MinIO, Redis, OTEL)
- CI/CD pipelines (PR gates, nightly builds)
- Testing strategy and documentation
- Contribution guidelines and templates

### Infrastructure
- ClickHouse for metrics and traces storage
- PostgreSQL for metadata
- MinIO for artifact storage (S3-compatible)
- Redis for queue and cache
- OpenTelemetry Collector for observability

---

## Version History

### Alpha Releases

Alpha releases use the format `0.1.0-alpha.N` where N is the alpha iteration.

| Version | Date | Notes |
|---------|------|-------|
| 0.1.0-alpha.1 | TBD | First alpha release |

### Release Types

- **alpha**: Early development, API may change significantly
- **beta**: Feature complete, API stabilizing
- **rc**: Release candidate, bug fixes only
- **stable**: Production ready

---

## How to Update This Changelog

When preparing a release:

1. Move items from `[Unreleased]` to a new version section
2. Add the release date
3. Tag the commit with `v{version}` (e.g., `v0.1.0-alpha.1`)
4. Push the tag to trigger the release workflow

### Categories

- **Added**: New features
- **Changed**: Changes to existing functionality
- **Deprecated**: Features to be removed in future
- **Removed**: Removed features
- **Fixed**: Bug fixes
- **Security**: Security fixes
- **Infrastructure**: DevOps, CI/CD, dependencies
