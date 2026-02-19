"""MLRunX doctor command - Check setup and connectivity."""

import os
import urllib.error
import urllib.request
from pathlib import Path

from .init import CONFIG_FILE, load_config


def check_mark(passed: bool) -> str:
    """Return a check mark or X based on status."""
    return "[OK]" if passed else "[FAIL]"


def doctor(verbose: bool = False) -> bool:
    """Check MLRunX setup and connectivity.

    Performs the following checks:
    1. Configuration file exists
    2. API server is reachable
    3. Required Python packages are installed
    4. Spool directory is writable

    Args:
        verbose: Show detailed diagnostic information

    Returns:
        True if all checks pass, False otherwise
    """
    print("MLRunX Doctor - Checking your setup...")
    print("=" * 50)
    print()

    all_passed = True

    # Check 1: Configuration file
    config = load_config()
    config_exists = config is not None
    print(f"{check_mark(config_exists)} Configuration file ({CONFIG_FILE})")

    if config_exists and verbose and config is not None:
        print(f"   Project: {config.get('project', 'N/A')}")
        print(f"   API URL: {config.get('api_url', 'N/A')}")

    if not config_exists:
        print("   Run 'mlrunx init' to create a configuration file.")
        all_passed = False

    # Check 2: API server connectivity
    api_url = (config or {}).get("api_url", "http://localhost:3001")
    api_reachable, api_version = check_api_connectivity(api_url)
    print(f"{check_mark(api_reachable)} API server ({api_url})")

    if api_reachable and verbose:
        print(f"   Version: {api_version}")

    if not api_reachable:
        print("   Make sure the MLRunX server is running:")
        print("   docker-compose up -d")
        all_passed = False

    # Check 3: Health endpoint
    health_ok = check_health_endpoint(api_url)
    print(f"{check_mark(health_ok)} Health endpoint")

    if not health_ok and api_reachable:
        print("   Server is reachable but health check failed.")
        all_passed = False

    # Check 4: Required packages
    packages_ok, missing = check_required_packages()
    print(f"{check_mark(packages_ok)} Required Python packages")

    if not packages_ok:
        print(f"   Missing: {', '.join(missing)}")
        print("   Install with: pip install mlrunx")
        all_passed = False

    # Check 5: Spool directory
    spool_dir = Path((config or {}).get("sdk", {}).get("spool_dir", ".mlrunx/spool"))
    spool_ok = check_spool_directory(spool_dir)
    print(f"{check_mark(spool_ok)} Spool directory ({spool_dir})")

    if not spool_ok:
        print(f"   Cannot write to spool directory: {spool_dir}")
        all_passed = False

    # Check 6: Environment variables
    env_vars = check_environment_variables()
    has_env_vars = len(env_vars) > 0
    print(f"[INFO] Environment variables: {len(env_vars)} set")

    if verbose and has_env_vars:
        for key, value in env_vars.items():
            # Mask sensitive values
            display_value = value[:4] + "..." if len(value) > 4 else value
            print(f"   {key}={display_value}")

    print()
    print("=" * 50)

    if all_passed:
        print("All checks passed! MLRunX is ready to use.")
        return True
    else:
        print("Some checks failed. Please fix the issues above.")
        return False


def check_api_connectivity(api_url: str) -> tuple[bool, str | None]:
    """Check if the API server is reachable.

    Args:
        api_url: URL of the MLRunX API

    Returns:
        Tuple of (is_reachable, version_string)
    """
    try:
        req = urllib.request.Request(f"{api_url}/", method="GET")
        req.add_header("Accept", "text/plain")
        with urllib.request.urlopen(req, timeout=5) as response:
            body = response.read().decode("utf-8")
            return True, body.strip()
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError):
        return False, None


def check_health_endpoint(api_url: str) -> bool:
    """Check if the health endpoint is responding.

    Args:
        api_url: URL of the MLRunX API

    Returns:
        True if health check passes
    """
    try:
        req = urllib.request.Request(f"{api_url}/health", method="GET")
        with urllib.request.urlopen(req, timeout=5) as response:
            return int(getattr(response, "status", 0)) == 200
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError):
        return False


def check_required_packages() -> tuple[bool, list[str]]:
    """Check if required Python packages are installed.

    Returns:
        Tuple of (all_installed, missing_packages)
    """
    required = ["pyyaml"]
    missing = []

    for package in required:
        try:
            __import__(package.replace("-", "_"))
        except ImportError:
            missing.append(package)

    return len(missing) == 0, missing


def check_spool_directory(spool_dir: Path) -> bool:
    """Check if the spool directory is writable.

    Args:
        spool_dir: Path to the spool directory

    Returns:
        True if directory exists and is writable
    """
    try:
        spool_dir.mkdir(parents=True, exist_ok=True)
        test_file = spool_dir / ".write_test"
        test_file.write_text("test")
        test_file.unlink()
        return True
    except OSError:
        return False


def check_environment_variables() -> dict[str, str]:
    """Check for MLRunX-related environment variables.

    Returns:
        Dictionary of MLRUNX_ prefixed environment variables
    """
    return {
        key: value
        for key, value in os.environ.items()
        if key.startswith("MLRUNX_")
    }
