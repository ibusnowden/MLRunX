# MLRunX Protocol Buffers

This directory contains the gRPC protocol buffer definitions for MLRunX services.

## Structure

```
proto/
└── mlrunx/v1/
    ├── common.proto   # Shared types (identifiers, metrics, errors)
    ├── ingest.proto   # Ingest service (data ingestion from SDKs)
    └── query.proto    # Query service (read access for UI/clients)
```

## Documentation

**Important**: The specification documents in `/docs/spec/` are the source of truth.
Proto definitions must match the docs, not vice versa.

| Proto | Spec Document |
|-------|---------------|
| `common.proto` | [limits.md](/docs/spec/limits.md) |
| `ingest.proto` | [ingest.md](/docs/spec/ingest.md) |
| `query.proto` | [query.md](/docs/spec/query.md) |

## Making Changes

> **Any change to proto files requires updating the corresponding spec document first.**

1. Update the spec document in `/docs/spec/`
2. Update the proto to match the spec
3. Ensure proto compiles: `make proto-check`
4. Update generated code: `make proto-gen`

## Compiling Protos

```bash
# Check that protos compile
protoc --proto_path=proto \
       --proto_path=/opt/homebrew/include \
       -o /dev/null \
       proto/mlrunx/v1/*.proto

# Generate Rust code (requires prost)
# (integrated into cargo build via build.rs)

# Generate Python stubs (requires grpcio-tools)
python -m grpc_tools.protoc \
       --proto_path=proto \
       --python_out=sdks/python/src \
       --grpc_python_out=sdks/python/src \
       proto/mlrunx/v1/*.proto
```

## Versioning

- Current version: `v1`
- Package: `mlrunx.v1`
- Go package: `github.com/mlrunx/mlrunx/gen/go/mlrunx/v1`

Breaking changes require a new version (e.g., `v2`).
