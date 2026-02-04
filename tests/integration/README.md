# MLRunX Integration Tests

End-to-end integration tests for the MLRunX API.

## Quick Start

### Option 1: Run against local API (recommended for development)

1. Start the API server in dev mode:
   ```bash
   cd apps/api
   MLRUNX_AUTH_DISABLED=true cargo run
   ```

2. Run the tests:
   ```bash
   cd tests/integration
   pip install -r requirements.txt
   python runner.py --api-url http://localhost:3001
   ```

### Option 2: Run with Docker Compose

1. Start the full test stack:
   ```bash
   docker compose -f docker-compose.test.yml up -d --wait
   ```

2. Run the tests:
   ```bash
   python runner.py
   ```

3. Cleanup:
   ```bash
   docker compose -f docker-compose.test.yml down -v
   ```

## Test Runner Options

```bash
# List available tests
python runner.py --list

# Run specific test(s)
python runner.py --test test_sdk_basic_logging
python runner.py --test test_health_check --test test_run_listing_and_filtering

# Custom API URL
python runner.py --api-url http://localhost:3001

# Skip waiting for API to be ready
python runner.py --no-wait

# Custom timeout for API startup
python runner.py --timeout 120
```

## Test Cases

| Test | Description |
|------|-------------|
| `test_health_check` | Verify API health endpoint |
| `test_sdk_basic_logging` | Init run, log metrics, finish |
| `test_run_listing_and_filtering` | List runs with project/status filters |
| `test_idempotent_batch_ingestion` | Verify duplicate batches are handled |
| `test_large_batch_ingestion` | Ingest 1000 metrics in single batch |
| `test_run_tags_update` | Add/update tags during run |
| `test_concurrent_runs` | Multiple concurrent runs |

## CI Integration

Integration tests run automatically in CI after the Rust build succeeds. See `.github/workflows/ci.yml` for details.

## Writing New Tests

Add test functions to `runner.py`:

```python
def test_my_feature(client: MLRunTestClient) -> None:
    """Test description here."""
    # Use assertions to verify behavior
    resp = client.init_run(project="test")
    assert resp["run_id"], "Should return run ID"

    # Clean up if needed
    client.finish_run(resp["run_id"])
```

Then add the function to `ALL_TESTS` list.
