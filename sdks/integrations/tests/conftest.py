"""Pytest configuration for MLRunX integrations tests."""

import sys
from pathlib import Path

# Add src directories to path
sdk_src = Path(__file__).parent.parent.parent / "python" / "src"
integrations_src = Path(__file__).parent.parent / "src"
sys.path.insert(0, str(sdk_src))
sys.path.insert(0, str(integrations_src))
