#!/usr/bin/env python3
"""Checkout entry point for the installed artifact staging helper."""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "mcp-server/src"))
from erebus_mcp.metropolis_setup.stage_artifacts import *  # noqa: F403

if __name__ == "__main__":
    raise SystemExit(main())
