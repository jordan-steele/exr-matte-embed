import os
import OpenEXR
import Imath
import multiprocessing
import time
import sys

class EXRProcessor:
    COMPRESSION_OPTIONS = ['none', 'rle', 'zip', 'zips', 'piz', 'pxr24', 'b44', 'b44a', 'dwaa']

    def __init__(self):
        if sys.platform == 'darwin':  # macOS
            multiprocessing.set_start_method('fork', force=True)
        elif sys.platform == 'win32':  # Windows
            multiprocessing.freeze_support()
            multiprocessing.set_start_method('spawn', force=True)
            os.environ['PYTHONUNBUFFERED'] = '1'

    def find_matching_pairs(self, main_folder, rgb_mode=False):
        """Find matching main/matte folder pairs"""
        pairs = []
        warnings = []
        
        if rgb_mode:
            # Look for RGB matte folders (_matteR, _matteG, _matteB, _matteA)
            processed_bases = set()
            
            for root, dirs, files in os.walk(main_folder):
                # Check if this is a matte folder
                if any(root.endswith(suffix) for suffix in ['_matteR', '_matteG', '_matteB', '_matteA']):
                    # Extract base folder name
                    for suffix in ['_matteR', '_matteG', '_matteB', '_matteA']:
                        if root.endswith(suffix):
                            base_folder = root[:-len(suffix)]
                            break
                    
                    # Skip if we've already processed this base or if base doesn't exist
                    if base_folder in processed_bases or not os.path.exists(base_folder):
                        continue
                    
                    processed_bases.add(base_folder)
                    
                    # Find all available matte channels
                    matte_folders = {}
                    for channel in ['R', 'G', 'B', 'A']:
                        potential_folder = base_folder + f'_matte{channel}'
                        if os.path.exists(potential_folder):
                            matte_folders[channel] = potential_folder

                    if not matte_folders:
                        continue

                    # Get files from base folder
                    try:
                        base_files = sorted([f for f in os.listdir(base_folder) if f.endswith('.exr')])
                        if not base_files:
                            warnings.append(f"No EXR files found in base folder: {base_folder}")
                            continue
                    except OSError as e:
                        warnings.append(f"Error reading base folder {base_folder}: {str(e)}")
                        continue

                    # Get files from each matte folder and verify counts match
                    matte_files = {}
                    file_count_mismatch = False
                    
                    for channel, folder in matte_folders.items():
                        try:
                            channel_files = sorted([f for f in os.listdir(folder) if f.endswith('.exr')])
                            if len(channel_files) != len(base_files):
                                warnings.append(f"File count mismatch for {base_folder}_matte{channel}: expected {len(base_files)}, found {len(channel_files)}")
                                file_count_mismatch = True
                                break
                            matte_files[channel] = channel_files
                        except OSError as e:
                            warnings.append(f"Error reading matte folder {folder}: {str(e)}")
                            file_count_mismatch = True
                            break

                    if file_count_mismatch:
                        continue

                    pairs.append({
                        'base_folder': base_folder,
                        'matte_folders': matte_folders,
                        'base_files': base_files,
                        'matte_files': matte_files,
                        'channels': list(matte_folders.keys())
                    })
        else:
            # Look for regular single-channel matte folders (_matte)
            for root, dirs, files in os.walk(main_folder):
                if root.endswith('_matte'):
                    base_folder = root[:-6]  # Remove '_matte' suffix
                    
                    if not os.path.exists(base_folder):
                        warnings.append(f"Base folder not found for matte folder: {root}")
                        continue
                    
                    try:
                        base_files = sorted([f for f in os.listdir(base_folder) if f.endswith('.exr')])
                        matte_files = sorted([f for f in os.listdir(root) if f.endswith('.exr')])
                        
                        if not base_files:
                            warnings.append(f"No EXR files found in base folder: {base_folder}")
                            continue
                            
                        if not matte_files:
                            warnings.append(f"No EXR files found in matte folder: {root}")
                            continue
                        
                        if len(base_files) != len(matte_files):
                            warnings.append(f"File count mismatch for {base_folder}: base has {len(base_files)}, matte has {len(matte_files)}")
                            continue

                        pairs.append({
                            'base_folder': base_folder,
                            'matte_folder': root,
                            'base_files': base_files,
                            'matte_files': matte_files
                        })
                    except OSError as e:
                        warnings.append(f"Error reading folders for {base_folder}: {str(e)}")
                        continue
                        
        return pairs, warnings

    def process_exr_file(self, base_folder, matte_info, base_file, matte_files, compression, rgb_mode, matte_channel_name):
        """Process a single EXR file with its matte"""
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

        if rgb_mode:
            # Process RGB matte channels
            matte_channels = {}
            available_channels = list(matte_info.keys())

            for channel in available_channels:
                try:
                    matte_file = os.path.join(matte_info[channel], matte_files[channel])
                    exr_matte = OpenEXR.InputFile(matte_file)
                    matte_channels[channel] = exr_matte.channel('R', Imath.PixelType(Imath.PixelType.HALF))
                    exr_matte.close()
                except Exception as e:
                    raise Exception(f"Error processing matte channel {channel}: {str(e)}")

            # Add matte channels to output
            for i, channel in enumerate(available_channels, 1):
                channel_name = f'{matte_channel_name}.{i}'
                header_out['channels'][channel_name] = Imath.Channel(Imath.PixelType(Imath.PixelType.HALF))
                channel_data[channel_name] = matte_channels[channel]
        else:
            # Process single matte channel
            try:
                exr2 = OpenEXR.InputFile(os.path.join(matte_info['matte_folder'], matte_files))
                header_out['channels'][matte_channel_name] = Imath.Channel(Imath.PixelType(Imath.PixelType.HALF))
                channel_data[matte_channel_name] = exr2.channel('R', Imath.PixelType(Imath.PixelType.HALF))
                exr2.close()
            except Exception as e:
                raise Exception(f"Error processing matte file: {str(e)}")

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
                                   num_processes, progress_queue, result_queue, stop_event):
        """Process sequences using cached scan results"""
        
        regular_pairs = scan_results.get('regular_pairs', [])
        rgb_pairs = scan_results.get('rgb_pairs', [])
        warnings = scan_results.get('warnings', [])
        
        all_pairs = regular_pairs + rgb_pairs
        
        if not all_pairs:
            progress_queue.put({'progress': 0, 'status1': "No sequences to process.", 'status2': ""})
            result_queue.put({'success': True})
            stop_event.set()
            return

        total_files = sum(len(pair['base_files']) for pair in all_pairs)
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
            
            # Process regular pairs
            for pair in regular_pairs:
                base_folder = pair['base_folder']
                matte_info = {'matte_folder': pair['matte_folder']}
                
                for i, base_file in enumerate(pair['base_files']):
                    matte_file = pair['matte_files'][i]
                    tasks.append((
                        base_folder,
                        matte_info,
                        base_file,
                        matte_file,
                        compression,
                        False,  # rgb_mode = False for regular pairs
                        matte_channel_name
                    ))
            
            # Process RGB pairs
            for pair in rgb_pairs:
                base_folder = pair['base_folder']
                matte_info = pair['matte_folders']
                
                for i, base_file in enumerate(pair['base_files']):
                    matte_files = {k: v[i] for k, v in pair['matte_files'].items()}
                    tasks.append((
                        base_folder,
                        matte_info,
                        base_file,
                        matte_files,
                        compression,
                        True,  # rgb_mode = True for RGB pairs
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
            result_queue.put({'success': True})

        stop_event.set()

    def process_sequences(self, folder, compression, rgb_mode, matte_channel_name, 
                        num_processes, progress_queue, result_queue, stop_event):
        """Legacy method for backward compatibility - auto-detects mode and processes"""
        
        # Scan for both types and auto-detect mode
        regular_pairs, regular_warnings = self.find_matching_pairs(folder, rgb_mode=False)
        rgb_pairs, rgb_warnings = self.find_matching_pairs(folder, rgb_mode=True)
        
        # Create scan results structure
        scan_results = {
            'regular_pairs': regular_pairs,
            'rgb_pairs': rgb_pairs,
            'warnings': regular_warnings + rgb_warnings,
            'total_sequences': len(regular_pairs) + len(rgb_pairs),
            'total_files': sum(len(pair['base_files']) for pair in regular_pairs + rgb_pairs)
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
