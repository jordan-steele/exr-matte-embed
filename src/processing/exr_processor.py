import os
import OpenEXR
import Imath
import multiprocessing
import time
import sys

class EXRProcessor:
    COMPRESSION_OPTIONS = ['none', 'rle', 'zip', 'zips', 'piz', 'pxr24', 'b44', 'b44a', 'dwaa']

    def __init__(self):
        pass

    def find_matching_pairs(self, main_folder, rgb_mode=False):
        pairs = []
        if rgb_mode:
            for root, dirs, files in os.walk(main_folder):
                if root.endswith('_matteR'):
                    base_folder = root[:-7]  # Remove '_matteR'
                    if not os.path.exists(base_folder):
                        continue
                        
                    matte_folders = {
                        'R': root,
                        'G': root[:-1] + 'G',
                        'B': root[:-1] + 'B',
                        'A': root[:-1] + 'A'
                    }
                    
                    # Check if all matte folders exist
                    if not all(os.path.exists(folder) for folder in matte_folders.values()):
                        continue

                    base_files = sorted([f for f in os.listdir(base_folder) if f.endswith('.exr')])
                    matte_files = {
                        channel: sorted([f for f in os.listdir(folder) if f.endswith('.exr')])
                        for channel, folder in matte_folders.items()
                    }

                    # Ensure all folders have the same number of files
                    file_counts = [len(files) for files in matte_files.values()]
                    if len(base_files) != file_counts[0] or len(set(file_counts)) != 1:
                        print(f"Warning: Mismatch in number of files for {base_folder}")
                        continue

                    pairs.append({
                        'base_folder': base_folder,
                        'matte_folders': matte_folders,
                        'base_files': base_files,
                        'matte_files': matte_files
                    })
        else:
            # Original single matte logic
            for root, dirs, files in os.walk(main_folder):
                if root.endswith('_matte'):
                    base_folder = root[:-6]
                    if os.path.exists(base_folder):
                        base_files = sorted([f for f in os.listdir(base_folder) if f.endswith('.exr')])
                        matte_files = sorted([f for f in os.listdir(root) if f.endswith('.exr')])
                        
                        if len(base_files) != len(matte_files):
                            print(f"Warning: Mismatch in number of files for {base_folder}")
                            continue

                        pairs.append({
                            'base_folder': base_folder,
                            'matte_folder': root,
                            'base_files': base_files,
                            'matte_files': matte_files
                        })
        return pairs

    def process_exr_file(self, base_folder, matte_info, base_file, matte_files, compression, rgb_mode, matte_channel_name):
        try:
            exr1 = OpenEXR.InputFile(os.path.join(base_folder, base_file))
        except Exception as e:
            raise Exception(f"Error opening base file: {str(e)}")

        header1 = exr1.header()
        header_out = OpenEXR.Header(
            header1['dataWindow'].max.x - header1['dataWindow'].min.x + 1,
            header1['dataWindow'].max.y - header1['dataWindow'].min.y + 1
        )

        for attribute, value in header1.items():
            if attribute != 'writer':
                header_out[attribute] = value

        channel_data = {
            channel: exr1.channel(channel, Imath.PixelType(Imath.PixelType.HALF))
            for channel in header1['channels'] 
            if not channel.startswith(matte_channel_name)
        }

        if rgb_mode:
            matte_channels = {}
            for channel in ['R', 'G', 'B', 'A']:
                try:
                    matte_file = os.path.join(matte_info['matte_folders'][channel], matte_files[channel])
                    exr_matte = OpenEXR.InputFile(matte_file)
                    matte_channels[channel] = exr_matte.channel('R', Imath.PixelType(Imath.PixelType.HALF))
                    exr_matte.close()
                except Exception as e:
                    raise Exception(f"Error processing matte channel {channel}: {str(e)}")

            for i, channel in enumerate(['R', 'G', 'B', 'A'], 1):
                header_out['channels'][f'{matte_channel_name}.{i}'] = Imath.Channel(Imath.PixelType(Imath.PixelType.HALF))
                channel_data[f'{matte_channel_name}.{i}'] = matte_channels[channel]
        else:
            try:
                exr2 = OpenEXR.InputFile(os.path.join(matte_info['matte_folder'], matte_files))
                header_out['channels'][matte_channel_name] = Imath.Channel(Imath.PixelType(Imath.PixelType.HALF))
                channel_data[matte_channel_name] = exr2.channel('R', Imath.PixelType(Imath.PixelType.HALF))
                exr2.close()
            except Exception as e:
                raise Exception(f"Error processing matte file: {str(e)}")

        header_out['compression'] = Imath.Compression(self.COMPRESSION_OPTIONS.index(compression))

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
        try:
            self.process_exr_file(*args)
            return args[0], args[1], args[2], None
        except Exception as e:
            return args[0], args[1], args[2], str(e)

    def process_sequences(self, folder, compression, rgb_mode, matte_channel_name, 
                        num_processes, progress_queue, result_queue, stop_event):
        
        if sys.platform == 'darwin':  # macOS
            multiprocessing.set_start_method('fork', force=True)

        pairs = self.find_matching_pairs(folder, rgb_mode)
        if not pairs:
            progress_queue.put({'progress': 0, 'status1': "No matching pairs found.", 'status2': ""})
            stop_event.set()
            return

        total_files = sum(len(pair['base_files']) for pair in pairs)
        processed_files = 0
        error_files = []

        start_time = time.time()

        with multiprocessing.Pool(processes=num_processes) as pool:
            tasks = []
            for pair_index, pair in enumerate(pairs, 1):
                base_folder = pair['base_folder']
                matte_info = pair['matte_folders'] if rgb_mode else {'matte_folder': pair['matte_folder']}
                
                for i, base_file in enumerate(pair['base_files']):
                    matte_files = ({k: v[i] for k, v in pair['matte_files'].items()} if rgb_mode 
                                 else pair['matte_files'][i])
                    tasks.append((
                        base_folder,
                        matte_info,
                        base_file,
                        matte_files,
                        compression,
                        rgb_mode,
                        matte_channel_name
                    ))

            for result in pool.imap_unordered(self.process_exr_file_wrapper, tasks):
                processed_files += 1
                base_folder, _, base_file, error = result

                if error:
                    error_files.append((base_file, str(error)))

                progress = (processed_files / total_files) * 100
                status1 = f"Processing folder: {os.path.basename(base_folder)}"
                status2 = f"Total progress: {processed_files}/{total_files}"
                progress_queue.put({
                    'progress': progress, 
                    'status1': status1, 
                    'status2': status2,
                    'processed': processed_files
                })

                elapsed_time = time.time() - start_time
                avg_time_per_file = elapsed_time / processed_files
                estimated_time_left = avg_time_per_file * (total_files - processed_files)
                progress_queue.put({
                    'timing': f"Elapsed: {elapsed_time:.2f}s, Avg: {avg_time_per_file:.2f}s/file, Est. remaining: {estimated_time_left:.2f}s"
                })

        if error_files:
            error_message = "The following files encountered errors:\n\n"
            for file, error in error_files:
                error_message += f"{file}: {error}\n"
            result_queue.put({'error_files': error_files, 'error_message': error_message})
        else:
            result_queue.put({'success': True})

        stop_event.set()

if __name__ == '__main__':
    multiprocessing.freeze_support()