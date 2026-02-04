"""Pytest configuration for MLRunX Python SDK tests."""

import sys
from pathlib import Path

# Add src directory to path so tests can import mlrunx
src_path = Path(__file__).parent.parent / "src"
sys.path.insert(0, str(src_path))
