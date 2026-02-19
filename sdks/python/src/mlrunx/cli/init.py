"""MLRunX init command - Initialize a project."""

from pathlib import Path
from typing import Any, cast

import yaml  # type: ignore[import-untyped]

CONFIG_FILE = ".mlrunx.yaml"
CONFIG_TEMPLATE = """\
# MLRunX Configuration
# Documentation: https://mlrunx.dev/docs/configuration

project: {project_name}
api_url: {api_url}

# SDK Settings
sdk:
  # Batch settings for metric logging
  batch_size: 100
  flush_interval_ms: 1000

  # Compression (gzip for HTTP transport)
  compression: true

  # Offline mode settings
  spool_enabled: true
  spool_dir: .mlrunx/spool

# Optional: Default tags for all runs
# tags:
#   environment: development
#   team: ml-team
"""


def init_project(
    project_name: str | None = None,
    api_url: str = "http://localhost:3001",
    force: bool = False,
) -> bool:
    """Initialize MLRunX in the current directory.

    Creates a .mlrunx.yaml configuration file with project settings.

    Args:
        project_name: Project name (defaults to current directory name)
        api_url: MLRunX API URL
        force: Overwrite existing configuration

    Returns:
        True if initialization succeeded, False otherwise
    """
    config_path = Path(CONFIG_FILE)

    # Check if config already exists
    if config_path.exists() and not force:
        print(f"Error: {CONFIG_FILE} already exists.")
        print("Use --force to overwrite.")
        return False

    # Default project name to directory name
    if project_name is None:
        project_name = Path.cwd().name

    # Validate project name
    if not project_name or not project_name.replace("-", "").replace("_", "").isalnum():
        print(f"Error: Invalid project name: {project_name}")
        print("Project names must be alphanumeric (with hyphens and underscores allowed).")
        return False

    # Create config content
    config_content = CONFIG_TEMPLATE.format(
        project_name=project_name,
        api_url=api_url,
    )

    # Write config file
    try:
        config_path.write_text(config_content)
    except OSError as e:
        print(f"Error: Failed to write {CONFIG_FILE}: {e}")
        return False

    # Create spool directory
    spool_dir = Path(".mlrunx/spool")
    try:
        spool_dir.mkdir(parents=True, exist_ok=True)
    except OSError as e:
        print(f"Warning: Failed to create spool directory: {e}")

    # Create .gitignore for .mlrunx directory
    gitignore_path = Path(".mlrunx/.gitignore")
    try:
        gitignore_path.parent.mkdir(parents=True, exist_ok=True)
        gitignore_path.write_text("# Ignore spool files (local offline data)\nspool/\n")
    except OSError:
        pass  # Not critical

    print(f"Initialized MLRunX project: {project_name}")
    print(f"Configuration: {config_path.absolute()}")
    print()
    print("Next steps:")
    print("  1. Start the MLRunX server (if not running):")
    print("     docker-compose up -d")
    print()
    print("  2. Add tracking to your training script:")
    print("     import mlrunx")
    print(f'     with mlrunx.start_run(project="{project_name}") as run:')
    print('         run.log({"loss": 0.5, "accuracy": 0.9})')
    print()
    print("  3. View your runs:")
    print(f"     {api_url}/")

    return True


def load_config() -> dict[str, Any] | None:
    """Load MLRunX configuration from .mlrunx.yaml.

    Returns:
        Configuration dictionary, or None if not found
    """
    config_path = Path(CONFIG_FILE)

    if not config_path.exists():
        # Check parent directories
        current = Path.cwd()
        while current != current.parent:
            candidate = current / CONFIG_FILE
            if candidate.exists():
                config_path = candidate
                break
            current = current.parent
        else:
            return None

    try:
        with open(config_path) as f:
            loaded = yaml.safe_load(f)
            if isinstance(loaded, dict):
                return cast(dict[str, Any], loaded)
            return None
    except Exception:
        return None
