"""MLRunX CLI - Command-line interface for MLRunX."""

import argparse
import sys

from .doctor import doctor
from .init import init_project


def main():
    """Main entry point for the mlrunx CLI."""
    parser = argparse.ArgumentParser(
        prog="mlrunx",
        description="MLRunX - ML Experiment Tracking CLI",
    )
    parser.add_argument(
        "--version",
        action="version",
        version="mlrunx 0.1.0",
    )

    subparsers = parser.add_subparsers(dest="command", help="Available commands")

    # init command
    init_parser = subparsers.add_parser(
        "init",
        help="Initialize MLRunX in the current directory",
    )
    init_parser.add_argument(
        "--project",
        "-p",
        type=str,
        help="Project name (defaults to directory name)",
    )
    init_parser.add_argument(
        "--api-url",
        type=str,
        default="http://localhost:3001",
        help="MLRunX API URL (default: http://localhost:3001)",
    )
    init_parser.add_argument(
        "--force",
        "-f",
        action="store_true",
        help="Overwrite existing configuration",
    )

    # doctor command
    doctor_parser = subparsers.add_parser(
        "doctor",
        help="Check MLRunX setup and connectivity",
    )
    doctor_parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Show detailed diagnostic information",
    )

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        sys.exit(0)

    if args.command == "init":
        success = init_project(
            project_name=args.project,
            api_url=args.api_url,
            force=args.force,
        )
        sys.exit(0 if success else 1)

    elif args.command == "doctor":
        success = doctor(verbose=args.verbose)
        sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
