#!/usr/bin/env python3
"""
Test script for CLI functionality
"""

import sys
import os
import tempfile
import shutil

# Add project root to path
project_root = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, project_root)

from src.cli.cli_processor import CLIProcessor

def test_cli():
    """Test CLI functionality"""
    cli = CLIProcessor()
    
    # Test help
    print("Testing --help...")
    try:
        cli.run(['--help'])
    except SystemExit as e:
        if e.code == 0:
            print("✓ Help works correctly")
        else:
            print("✗ Help failed")
            return False
    
    # Test version
    print("\nTesting --version...")
    try:
        cli.run(['--version'])
    except SystemExit as e:
        if e.code == 0:
            print("✓ Version works correctly")
        else:
            print("✗ Version failed")
            return False
    
    # Test with non-existent folder
    print("\nTesting with non-existent folder...")
    result = cli.run(['/non/existent/folder'])
    if result == 1:
        print("✓ Properly handles non-existent folder")
    else:
        print("✗ Should return error code 1 for non-existent folder")
        return False
    
    # Test scan-only with empty folder
    print("\nTesting scan-only with empty folder...")
    with tempfile.TemporaryDirectory() as temp_dir:
        result = cli.run([temp_dir, '--scan-only'])
        if result == 0:
            print("✓ Properly handles empty folder")
        else:
            print("✗ Should return success for empty folder scan")
            return False
    
    print("\n✓ All CLI tests passed!")
    return True

if __name__ == '__main__':
    success = test_cli()
    sys.exit(0 if success else 1)
