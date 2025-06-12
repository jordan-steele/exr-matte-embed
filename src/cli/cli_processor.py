"""
CLI processor for EXR Matte Embed
Provides command-line interface without GUI dependencies
"""
import os
import sys
import argparse
import threading
import queue
import time
from ..processing.exr_processor import EXRProcessor
from version import get_version


class CLIProgress:
    """Simple progress tracker for CLI output"""
    
    def __init__(self, total_files=0):
        self.total_files = total_files
        self.processed_files = 0
        self.start_time = time.time()
        self.last_update = 0
        
    def update(self, processed=None, status1="", status2=""):
        """Update progress and print status"""
        if processed is not None:
            self.processed_files = processed
            
        current_time = time.time()
        
        # Only update every 0.5 seconds to avoid spam
        if current_time - self.last_update < 0.5 and processed != self.total_files:
            return
            
        self.last_update = current_time
        
        if self.total_files > 0:
            progress_pct = (self.processed_files / self.total_files) * 100
            elapsed = current_time - self.start_time
            
            if self.processed_files > 0:
                avg_time = elapsed / self.processed_files
                remaining_files = self.total_files - self.processed_files
                est_remaining = avg_time * remaining_files
                
                print(f"\rProgress: {self.processed_files}/{self.total_files} ({progress_pct:.1f}%) | "
                      f"Elapsed: {elapsed:.1f}s | Est. remaining: {est_remaining:.1f}s", end="")
            else:
                print(f"\rProgress: {self.processed_files}/{self.total_files} ({progress_pct:.1f}%)", end="")
        else:
            print(f"\r{status1} {status2}", end="")
            
        # Always print newline when complete
        if processed == self.total_files:
            print()  # Final newline


class CLIProcessor:
    """Command-line interface for EXR processing"""
    
    def __init__(self):
        self.processor = EXRProcessor()
        
    def create_parser(self):
        """Create argument parser for CLI"""
        parser = argparse.ArgumentParser(
            description="EXR Matte Embed - Command Line Interface",
            formatter_class=argparse.RawDescriptionHelpFormatter,
            epilog="""
Examples:
  %(prog)s /path/to/sequences
  %(prog)s /path/to/sequences --compression zip --matte-channel alpha
  %(prog)s /path/to/sequences --processes 8 --replace-originals
  %(prog)s /path/to/sequences --scan-only
            """
        )
        
        parser.add_argument(
            'folder_path',
            help='Path to folder containing EXR sequences and their matte folders'
        )
        
        parser.add_argument(
            '--compression', '-c',
            choices=self.processor.COMPRESSION_OPTIONS,
            default='piz',
            help='Compression type for output EXR files (default: piz)'
        )
        
        parser.add_argument(
            '--matte-channel', '-m',
            default='matte',
            help='Name for the matte channel in output files (default: matte)'
        )
        
        parser.add_argument(
            '--processes', '-p',
            type=int,
            default=max(os.cpu_count() // 2, 1),
            help=f'Number of parallel processes (default: {max(os.cpu_count() // 2, 1)})'
        )
        
        parser.add_argument(
            '--replace-originals', '-r',
            action='store_true',
            help='Replace original folders (move to trash and rename embedded folders)'
        )
        
        parser.add_argument(
            '--scan-only', '-s',
            action='store_true',
            help='Only scan and report sequences, do not process'
        )
        
        parser.add_argument(
            '--quiet', '-q',
            action='store_true',
            help='Minimal output (errors and final status only)'
        )
        
        parser.add_argument(
            '--verbose', '-v',
            action='store_true',
            help='Verbose output with detailed progress'
        )
        
        parser.add_argument(
            '--version',
            action='version',
            version=f'EXR Matte Embed CLI v{get_version()}'
        )
        
        return parser
    
    def validate_args(self, args):
        """Validate command line arguments"""
        errors = []
        
        # Check folder exists
        if not os.path.exists(args.folder_path):
            errors.append(f"Folder does not exist: {args.folder_path}")
        elif not os.path.isdir(args.folder_path):
            errors.append(f"Path is not a directory: {args.folder_path}")
            
        # Check process count
        max_processes = os.cpu_count()
        if args.processes < 1:
            errors.append("Number of processes must be at least 1")
        elif args.processes > max_processes:
            errors.append(f"Number of processes cannot exceed {max_processes}")
            
        # Check conflicting options
        if args.quiet and args.verbose:
            errors.append("Cannot use both --quiet and --verbose options")
            
        return errors
    
    def print_scan_results(self, scan_results, quiet=False):
        """Print scan results to console"""
        if quiet:
            return
            
        pairs = scan_results.get('pairs', [])
        warnings = scan_results.get('warnings', [])
        
        print(f"\nScan Results:")
        print(f"Found {len(pairs)} sequence(s) with {scan_results.get('total_files', 0)} total files\n")
        
        if pairs:
            # Group by type for display
            single_channel = [p for p in pairs if len(p['channels']) == 1 and 'base' in p['channels']]
            multi_channel = [p for p in pairs if len(p['channels']) > 1 or 'base' not in p['channels']]
            
            if single_channel:
                print("Single Channel Matte Sequences:")
                for pair in single_channel:
                    base_name = os.path.basename(pair['base_folder'])
                    matte_folder = os.path.basename(pair['matte_folders']['base'])
                    print(f"  • {base_name} ({len(pair['base_files'])} files)")
                    print(f"    └─ Matte: {matte_folder}")
                print()
                
            if multi_channel:
                print("Multi-Channel Matte Sequences:")
                for pair in multi_channel:
                    base_name = os.path.basename(pair['base_folder'])
                    print(f"  • {base_name} ({len(pair['base_files'])} files)")
                    print(f"    └─ Type: {pair['sequence_type']}")
                    for channel in sorted(pair['channels']):
                        matte_folder = os.path.basename(pair['matte_folders'][channel])
                        if channel == 'base':
                            display_name = 'matte'
                        elif channel.lower() in ['r', 'g', 'b', 'a']:
                            display_name = f'matte.matte_{channel.lower()}'
                        else:
                            display_name = f'matte.{channel}'
                        print(f"    └─ {display_name}: {matte_folder}")
                print()
        
        if warnings:
            print("Warnings:")
            for warning in warnings:
                print(f"  ⚠ {warning}")
            print()
    
    def run_scan(self, folder_path, quiet=False):
        """Run scan and return results"""
        if not quiet:
            print(f"Scanning folder: {folder_path}")
            
        pairs, warnings = self.processor.find_matching_pairs(folder_path)
        
        scan_results = {
            'pairs': pairs,
            'warnings': warnings,
            'total_sequences': len(pairs),
            'total_files': sum(len(pair['base_files']) for pair in pairs)
        }
        
        return scan_results
    
    def run_processing(self, args, scan_results):
        """Run the processing with progress tracking"""
        if scan_results['total_sequences'] == 0:
            print("No sequences found to process.")
            return True
            
        if not args.quiet:
            print(f"\nStarting processing with {args.processes} processes...")
            print(f"Compression: {args.compression}")
            print(f"Matte channel: {args.matte_channel}")
            if args.replace_originals:
                print("Replace originals: YES (originals will be moved to trash)")
            print()
        
        # Set up progress tracking
        progress_queue = queue.Queue()
        result_queue = queue.Queue()
        stop_event = threading.Event()
        
        progress_tracker = CLIProgress(scan_results['total_files'])
        
        # Start processing in separate thread
        processing_thread = threading.Thread(
            target=self.processor.process_sequences_from_cache,
            args=(
                scan_results,
                args.compression,
                args.matte_channel,
                args.processes,
                progress_queue,
                result_queue,
                stop_event,
                args.replace_originals
            )
        )
        
        processing_thread.start()
        
        # Monitor progress
        while processing_thread.is_alive():
            try:
                progress_data = progress_queue.get(timeout=0.1)
                if 'progress' in progress_data:
                    processed = progress_data.get('processed', progress_tracker.processed_files)
                    if not args.quiet:
                        progress_tracker.update(
                            processed=processed,
                            status1=progress_data.get('status1', ''),
                            status2=progress_data.get('status2', '')
                        )
            except queue.Empty:
                continue
                
        # Wait for thread to complete
        processing_thread.join()
        
        # Get final result
        try:
            result = result_queue.get_nowait()
        except queue.Empty:
            print("\nError: No result returned from processing")
            return False
            
        # Handle results
        if result.get('error'):
            print(f"\nError during processing: {result.get('error_message', 'Unknown error')}")
            return False
        elif result.get('error_files') or result.get('warnings'):
            if result.get('warnings'):
                print(f"\nProcessing completed with warnings:")
                for warning in result['warnings']:
                    print(f"  ⚠ {warning}")
                    
            if result.get('error_files'):
                print(f"\nProcessing completed with errors:")
                for file, error in result['error_files']:
                    print(f"  ✗ {file}: {error}")
            return False
        else:
            if not args.quiet:
                if result.get('replaced_originals'):
                    processed_count = len(result.get('processed_pairs', []))
                    print(f"\n✓ All files processed successfully!")
                    print(f"✓ Originals moved to trash and {processed_count} sequence(s) replaced.")
                else:
                    print(f"\n✓ All files processed successfully!")
            return True
    
    def run(self, args=None):
        """Main CLI entry point"""
        parser = self.create_parser()
        args = parser.parse_args(args)
        
        # Validate arguments
        errors = self.validate_args(args)
        if errors:
            print("Error(s):", file=sys.stderr)
            for error in errors:
                print(f"  {error}", file=sys.stderr)
            return 1
            
        try:
            # Run scan
            scan_results = self.run_scan(args.folder_path, args.quiet)
            
            # Print scan results
            self.print_scan_results(scan_results, args.quiet)
            
            # Check if we found anything
            if scan_results['total_sequences'] == 0:
                if not args.quiet:
                    print("No matching EXR sequences found.")
                return 0
                
            # If scan-only mode, exit here
            if args.scan_only:
                if not args.quiet:
                    print("Scan complete (scan-only mode).")
                return 0
                
            # Run processing
            success = self.run_processing(args, scan_results)
            return 0 if success else 1
            
        except KeyboardInterrupt:
            print("\nOperation cancelled by user.")
            return 130
        except Exception as e:
            print(f"Unexpected error: {e}", file=sys.stderr)
            if args.verbose:
                import traceback
                traceback.print_exc()
            return 1


def main():
    """Entry point for CLI"""
    cli = CLIProcessor()
    sys.exit(cli.run())


if __name__ == '__main__':
    main()
