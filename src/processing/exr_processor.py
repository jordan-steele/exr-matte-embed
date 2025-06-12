import os
import OpenEXR
import Imath
import multiprocessing
import time
import sys
import re
from send2trash import send2trash

class EXRProcessor:
    COMPRESSION_OPTIONS = ['none', 'rle', 'zip', 'zips', 'piz', 'pxr24', 'b44', 'b44a', 'dwaa']

    def __init__(self):
        if sys.platform == 'darwin':  # macOS
            multiprocessing.set_start_method('fork', force=True)
        elif sys.platform == 'win32':  # Windows
            multiprocessing.freeze_support()
            multiprocessing.set_start_method('spawn', force=True)
            os.environ['PYTHONUNBUFFERED'] = '1'

    def extract_frame_numbers(self, file_list):
        """Extract frame numbers from a list of EXR filenames"""
        frame_numbers = []
        frame_pattern = re.compile(r'\.(\d{4,})\.(exr)$', re.IGNORECASE)
        
        for filename in file_list:
            match = frame_pattern.search(filename)
            if match:
                frame_numbers.append(match.group(1))
            else:
                # Fallback: try to find any sequence of 4+ digits before .exr
                fallback_pattern = re.compile(r'(\d{4,})(?=.*\.exr$)', re.IGNORECASE)
                fallback_match = fallback_pattern.search(filename)
                if fallback_match:
                    frame_numbers.append(fallback_match.group(1))
                else:
                    # If no frame number found, use filename without extension as identifier
                    frame_numbers.append(os.path.splitext(filename)[0])
        
        return frame_numbers

    def validate_frame_sequences(self, base_files, matte_files_dict, base_folder):
        """Validate that all sequences have matching frame numbers"""
        warnings = []
        
        # Extract frame numbers from base sequence
        base_frames = set(self.extract_frame_numbers(base_files))
        
        # Check each matte sequence against base
        for channel_name, matte_files in matte_files_dict.items():
            matte_frames = set(self.extract_frame_numbers(matte_files))
            
            # Check for exact frame match
            if base_frames != matte_frames:
                missing_in_matte = base_frames - matte_frames
                extra_in_matte = matte_frames - base_frames
                
                base_name = os.path.basename(base_folder)
                channel_display = channel_name if channel_name != 'base' else 'matte'
                
                if missing_in_matte:
                    missing_str = ', '.join(sorted(missing_in_matte))
                    warnings.append(f"Channel '{channel_display}' in sequence '{base_name}' is missing frames: {missing_str}")
                
                if extra_in_matte:
                    extra_str = ', '.join(sorted(extra_in_matte))
                    warnings.append(f"Channel '{channel_display}' in sequence '{base_name}' has extra frames: {extra_str}")
                
                return False, warnings
        
        return True, warnings

    def find_matching_pairs(self, main_folder):
        """Find matching main/matte folder pairs using flexible _matte* detection"""
        pairs = []
        warnings = []
        
        # Dictionary to group sequences by base folder
        sequence_groups = {}
        
        # Walk through all folders to find matte folders
        for root, dirs, files in os.walk(main_folder):
            # Check if this folder matches the _matte* pattern
            folder_name = os.path.basename(root)
            matte_match = re.match(r'(.+)_matte(.*)$', folder_name)
            
            if matte_match:
                base_name = matte_match.group(1)
                matte_suffix = matte_match.group(2)
                base_folder = os.path.join(os.path.dirname(root), base_name)
                
                # Check if base folder exists
                if not os.path.exists(base_folder):
                    warnings.append(f"Base folder not found for matte folder: {root}")
                    continue
                
                # Initialize sequence group if not seen before
                if base_folder not in sequence_groups:
                    sequence_groups[base_folder] = {
                        'base_folder': base_folder,
                        'matte_folders': {},
                        'base_files': None
                    }
                
                # Determine channel name
                if matte_suffix == '':
                    # Just "_matte" - this is the base matte channel
                    channel_name = 'base'
                else:
                    # "_matte{suffix}" - use suffix as channel name (lowercase)
                    channel_name = matte_suffix.lower()
                
                # Store matte folder info
                sequence_groups[base_folder]['matte_folders'][channel_name] = root
        
        # Process each sequence group
        for base_folder, group_info in sequence_groups.items():
            try:
                # Get base files
                base_files = sorted([f for f in os.listdir(base_folder) if f.endswith('.exr')])
                if not base_files:
                    warnings.append(f"No EXR files found in base folder: {base_folder}")
                    continue
                
                group_info['base_files'] = base_files
                
                # Validate each matte folder and get file lists
                matte_files = {}
                file_count_mismatch = False
                
                for channel_name, matte_folder in group_info['matte_folders'].items():
                    try:
                        channel_files = sorted([f for f in os.listdir(matte_folder) if f.endswith('.exr')])
                        if len(channel_files) != len(base_files):
                            warnings.append(f"File count mismatch for {matte_folder}: expected {len(base_files)}, found {len(channel_files)}")
                            file_count_mismatch = True
                            break
                        matte_files[channel_name] = channel_files
                    except OSError as e:
                        warnings.append(f"Error reading matte folder {matte_folder}: {str(e)}")
                        file_count_mismatch = True
                        break
                
                if file_count_mismatch:
                    continue

                # Validate frame number sequences
                frames_valid, frame_warnings = self.validate_frame_sequences(base_files, matte_files, base_folder)
                warnings.extend(frame_warnings)
                
                if not frames_valid:
                    # Skip this sequence if frame numbers don't match
                    continue
                
                # Determine sequence type
                channel_names = list(group_info['matte_folders'].keys())
                if len(channel_names) == 1 and 'base' in channel_names:
                    sequence_type = "Single Channel Matte"
                else:
                    # Show actual channel names with conflict resolution
                    display_channels = []
                    for name in sorted(channel_names):
                        if name == 'base':
                            display_channels.append('matte')
                        elif name.lower() in ['r', 'g', 'b', 'a']:
                            display_channels.append(f'matte_{name.lower()}')
                        else:
                            display_channels.append(name)
                    sequence_type = f"Multi-Channel ({', '.join(display_channels)})"
                
                pairs.append({
                    'base_folder': base_folder,
                    'matte_folders': group_info['matte_folders'],
                    'base_files': base_files,
                    'matte_files': matte_files,
                    'channels': channel_names,
                    'sequence_type': sequence_type
                })
                
            except OSError as e:
                warnings.append(f"Error reading base folder {base_folder}: {str(e)}")
                continue
        
        return pairs, warnings

    def process_exr_file(self, base_folder, matte_info, base_file, matte_files, compression, matte_channel_name):
        """Process a single EXR file with its matte channels"""
        try:
            exr1 = OpenEXR.InputFile(os.path.join(base_folder, base_file))
        except Exception as e:
            raise Exception(f"Error opening base file: {str(e)}")

        header1 = exr1.header()
        header_out = OpenEXR.Header(
            header1['dataWindow'].max.x - header1['dataWindow'].min.x + 1,
            header1['dataWindow'].max.y - header1['dataWindow'].min.y + 1
        )

        # Copy all attributes except 'writer'
        for attribute, value in header1.items():
            if attribute != 'writer':
                header_out[attribute] = value

        # Copy all existing channels except existing matte channels
        channel_data = {
            channel: exr1.channel(channel, Imath.PixelType(Imath.PixelType.HALF))
            for channel in header1['channels'] 
            if not channel.startswith(matte_channel_name)
        }

        # First, collect all matte data
        matte_channels = {}
        for channel_name, matte_folder in matte_info.items():
            try:
                matte_file_path = os.path.join(matte_folder, matte_files[channel_name])
                exr_matte = OpenEXR.InputFile(matte_file_path)
                matte_data = exr_matte.channel('R', Imath.PixelType(Imath.PixelType.HALF))
                matte_channels[channel_name] = matte_data
                exr_matte.close()
            except Exception as e:
                raise Exception(f"Error processing matte channel {channel_name}: {str(e)}")

        # Then, add all matte channels to output
        for channel_name, matte_data in matte_channels.items():
            # Determine output channel name
            if channel_name == 'base':
                output_channel = matte_channel_name
            else:
                # Handle special cases that conflict with standard EXR channels
                if channel_name.lower() in ['r', 'g', 'b', 'a']:
                    # Add 'matte_' prefix to avoid conflicts with R, G, B, A channels
                    output_channel = f'{matte_channel_name}.matte_{channel_name.lower()}'
                else:
                    output_channel = f'{matte_channel_name}.{channel_name}'
            
            # Add to output
            header_out['channels'][output_channel] = Imath.Channel(Imath.PixelType(Imath.PixelType.HALF))
            channel_data[output_channel] = matte_data

        # Set compression
        header_out['compression'] = Imath.Compression(self.COMPRESSION_OPTIONS.index(compression))

        # Create output directory
        output_dir = base_folder + '_embedded'
        os.makedirs(output_dir, exist_ok=True)
        
        try:
            output_path = os.path.join(output_dir, base_file)
            exr_out = OpenEXR.OutputFile(output_path, header_out)
            exr_out.writePixels(channel_data)
            exr_out.close()
        except Exception as e:
            raise Exception(f"Error writing output file: {str(e)}")

        exr1.close()

    def process_exr_file_wrapper(self, args):
        """Wrapper for multiprocessing"""
        try:
            self.process_exr_file(*args)
            return args[0], args[1], args[2], None
        except Exception as e:
            return args[0], args[1], args[2], str(e)

    def process_sequences_from_cache(self, scan_results, compression, matte_channel_name, 
                                   num_processes, progress_queue, result_queue, stop_event, replace_originals=False):
        """Process sequences using cached scan results"""
        
        pairs = scan_results.get('pairs', [])
        warnings = scan_results.get('warnings', [])
        
        if not pairs:
            progress_queue.put({'progress': 0, 'status1': "No sequences to process.", 'status2': ""})
            result_queue.put({'success': True})
            stop_event.set()
            return

        total_files = sum(len(pair['base_files']) for pair in pairs)
        processed_files = 0
        error_files = []

        start_time = time.time()

        # Determine multiprocessing context
        if sys.platform == 'win32':
            mp_context = multiprocessing.get_context('spawn')
        else:
            mp_context = multiprocessing

        with mp_context.Pool(processes=num_processes) as pool:
            tasks = []
            
            # Create tasks for all pairs
            for pair in pairs:
                base_folder = pair['base_folder']
                matte_info = pair['matte_folders']
                
                for i, base_file in enumerate(pair['base_files']):
                    # Get corresponding matte files for this frame
                    matte_files = {channel: files[i] for channel, files in pair['matte_files'].items()}
                    
                    tasks.append((
                        base_folder,
                        matte_info,
                        base_file,
                        matte_files,
                        compression,
                        matte_channel_name
                    ))

            # Process files
            for result in pool.imap_unordered(self.process_exr_file_wrapper, tasks):
                if stop_event.is_set():
                    break
                    
                processed_files += 1
                base_folder, _, base_file, error = result

                if error:
                    error_files.append((base_file, str(error)))

                progress = (processed_files / total_files) * 100
                status1 = f"Processing: {os.path.basename(base_folder)}"
                status2 = f"Progress: {processed_files}/{total_files} files"
                progress_queue.put({
                    'progress': progress, 
                    'status1': status1, 
                    'status2': status2,
                    'processed': processed_files
                })

                # Update timing information
                elapsed_time = time.time() - start_time
                avg_time_per_file = elapsed_time / processed_files
                estimated_time_left = avg_time_per_file * (total_files - processed_files)
                progress_queue.put({
                    'timing': f"Elapsed: {elapsed_time:.2f}s, Avg: {avg_time_per_file:.2f}s/file, Est. remaining: {estimated_time_left:.2f}s"
                })

        # Handle original replacement if requested and no errors occurred
        processed_pairs = []
        if replace_originals and not error_files:
            try:
                progress_queue.put({
                    'progress': 100, 
                    'status1': 'Replacing original folders...', 
                    'status2': 'Moving originals to trash and renaming embedded folders'
                })
                
                for pair in pairs:
                    base_folder = pair['base_folder']
                    embedded_folder = base_folder + '_embedded'
                    
                    if os.path.exists(embedded_folder):
                        # Move original base folder to trash
                        send2trash(base_folder)
                        
                        # Move all matte folders to trash
                        for matte_folder in pair['matte_folders'].values():
                            if os.path.exists(matte_folder):
                                send2trash(matte_folder)
                        
                        # Rename embedded folder to original name
                        os.rename(embedded_folder, base_folder)
                        processed_pairs.append(base_folder)
                        
            except Exception as e:
                warnings.append(f"Error during original replacement: {str(e)}")
                error_files.append(("Replacement Process", str(e)))

        # Prepare results
        if error_files or warnings:
            error_message = ""
            
            if warnings:
                error_message += "The following warnings were encountered during scanning:\n\n"
                for warning in warnings:
                    error_message += f"WARNING: {warning}\n"
                error_message += "\n"
                
            if error_files:
                error_message += "The following files encountered errors during processing:\n\n"
                for file, error in error_files:
                    error_message += f"{file}: {error}\n"
                    
            result_queue.put({
                'error_files': error_files,
                'warnings': warnings,
                'error_message': error_message
            })
        else:
            success_result = {'success': True}
            if replace_originals and processed_pairs:
                success_result['replaced_originals'] = True
                success_result['processed_pairs'] = processed_pairs
            result_queue.put(success_result)

        stop_event.set()

    def process_sequences(self, folder, compression, rgb_mode, matte_channel_name, 
                        num_processes, progress_queue, result_queue, stop_event):
        """Legacy method for backward compatibility - auto-detects and processes"""
        
        # Scan using new flexible approach
        pairs, warnings = self.find_matching_pairs(folder)
        
        # Create scan results structure
        scan_results = {
            'pairs': pairs,
            'warnings': warnings,
            'total_sequences': len(pairs),
            'total_files': sum(len(pair['base_files']) for pair in pairs)
        }
        
        # Process using the new cached method
        return self.process_sequences_from_cache(
            scan_results, compression, matte_channel_name,
            num_processes, progress_queue, result_queue, stop_event
        )

if __name__ == '__main__':
    multiprocessing.freeze_support()
    if sys.platform == 'win32':
        multiprocessing.set_start_method('spawn', force=True)
