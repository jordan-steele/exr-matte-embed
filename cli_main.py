#!/usr/bin/env python3
"""
EXR Matte Embed - Command Line Interface
Entry point for CLI version without GUI dependencies
"""

import sys
import os

# Add the project root to Python path so we can import modules
project_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, project_root)

from src.cli.cli_processor import main

if __name__ == "__main__":
    main()
