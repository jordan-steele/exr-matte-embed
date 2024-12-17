import tkinter as tk
from tkinter import filedialog, ttk, messagebox
import threading
import queue
import multiprocessing
from ..utils.config import Config
import time

class ToolTip:
    def __init__(self, widget, text):
        self.widget = widget
        self.text = text
        self.tooltip = None
        self.widget.bind('<Enter>', self.enter)
        self.widget.bind('<Leave>', self.leave)

    def enter(self, event=None):
        x = self.widget.winfo_rootx()
        y = self.widget.winfo_rooty()
        x += 20
        y += 20
        
        self.tooltip = tk.Toplevel(self.widget)
        self.tooltip.wm_overrideredirect(True)
        self.tooltip.wm_geometry(f"+{x}+{y}")
        label = tk.Label(self.tooltip, text=self.text, justify='left',
                        borderwidth=1, padx=6, pady=6,
                        wraplength=300)
        label.pack()

    def leave(self, event=None):
        if self.tooltip:
            self.tooltip.destroy()
            self.tooltip = None

class EXRProcessorGUI:
    def __init__(self, master, processor):
        self.master = master
        self.processor = processor  # Instance of EXRProcessor class
        master.title("Batch Embed EXR Mattes")
        master.geometry("726x400")

        # Initialize variables
        self.folder_path = tk.StringVar()
        self.compression = tk.StringVar(value='piz')
        self.rgb_mode = tk.BooleanVar(value=False)
        self.matte_channel_name = tk.StringVar(value='matte')
        self.num_processes = tk.IntVar(value=max(multiprocessing.cpu_count() // 2, 1))

        # Progress tracking variables
        self.progress_var = tk.DoubleVar()
        self.progress_text1 = tk.StringVar()
        self.progress_text2 = tk.StringVar()
        self.timing_text = tk.StringVar()

        self.config = Config()
        self.load_config()

        # Create GUI elements
        self.create_widgets()

        # Initialize processing related attributes
        self.processing_thread = None
        self.progress_queue = queue.Queue()
        self.result_queue = queue.Queue()
        self.stop_event = threading.Event()

    def load_config(self):
        config_data = self.config.load()
        self.matte_channel_name.set(config_data['matte_channel_name'])

    def save_config(self):
        config_data = {
            'matte_channel_name': self.matte_channel_name.get()
        }
        self.config.save(config_data)

    def create_widgets(self):
        # Folder selection
        tk.Label(self.master, text="Select Folder:").grid(row=0, column=0, sticky='w', padx=10, pady=5)
        tk.Entry(self.master, textvariable=self.folder_path, width=50).grid(row=0, column=1, padx=5, pady=5)
        tk.Button(self.master, text="Browse", command=self.select_folder).grid(row=0, column=2, padx=5, pady=5)

        # Compression
        tk.Label(self.master, text="Compression:").grid(row=1, column=0, sticky='w', padx=10, pady=5)
        compression_dropdown = ttk.Combobox(self.master, textvariable=self.compression, 
                                          values=self.processor.COMPRESSION_OPTIONS)
        compression_dropdown.grid(row=1, column=1, sticky='w', padx=5, pady=5)

        # RGB Matte Mode with tooltip
        rgb_frame = tk.Frame(self.master)
        rgb_frame.grid(row=2, column=0, columnspan=2, sticky='w', padx=10, pady=5)
        
        rgb_cb = tk.Checkbutton(rgb_frame, text="RGB Matte Mode", variable=self.rgb_mode)
        rgb_cb.pack(side='left')
        
        help_canvas = tk.Canvas(rgb_frame, width=16, height=16, highlightthickness=0)
        help_canvas.pack(side='left', padx=5)
        help_canvas.create_oval(1, 1, 15, 15, width=1)
        help_canvas.create_text(8, 8, text="?", font=("Arial", 8, "bold"))
        
        tooltip_text = ("RGB Matte Mode expects source files to be named with _matteR, _matteG, _matteB,\n"
                       "and _matteA suffixes. It will embed four separate matte channels from these files.\n\n"
                       "For example: SHOW_100_010_020_v001_matteR/frame.001.exr, SHOW_100_010_020_v001_matteG/frame.001.exr, etc.")
        ToolTip(help_canvas, tooltip_text)

        # Matte Channel Name
        tk.Label(self.master, text="Matte Channel Name:").grid(row=3, column=0, sticky='w', padx=10, pady=5)
        matte_name_entry = tk.Entry(self.master, textvariable=self.matte_channel_name, width=20)
        matte_name_entry.grid(row=3, column=1, sticky='w', padx=5, pady=5)
        matte_name_entry.bind('<FocusOut>', lambda e: self.save_config())

        # Number of Processes
        tk.Label(self.master, text="Number of Processes:").grid(row=4, column=0, sticky='w', padx=10, pady=5)
        process_spinbox = tk.Spinbox(self.master, from_=1, to=multiprocessing.cpu_count(), 
                                   textvariable=self.num_processes, width=5)
        process_spinbox.grid(row=4, column=1, sticky='w', padx=5, pady=5)

        # Process Button
        self.process_button = tk.Button(self.master, text="Process", command=self.start_processing)
        self.process_button.grid(row=5, column=1, pady=20)

        # Progress Bar and Labels
        self.progress_bar = ttk.Progressbar(self.master, variable=self.progress_var, maximum=100)
        self.progress_bar.grid(row=6, column=0, columnspan=3, sticky='ew', padx=10, pady=5)

        self.progress_label1 = tk.Label(self.master, textvariable=self.progress_text1, anchor='w')
        self.progress_label1.grid(row=7, column=0, columnspan=3, sticky='w', padx=10, pady=2)
        self.progress_label2 = tk.Label(self.master, textvariable=self.progress_text2, anchor='w')
        self.progress_label2.grid(row=8, column=0, columnspan=3, sticky='w', padx=10, pady=2)

        self.timing_label = tk.Label(self.master, textvariable=self.timing_text, anchor='w')
        self.timing_label.grid(row=9, column=0, columnspan=3, sticky='w', padx=10, pady=2)

    def select_folder(self):
        folder = filedialog.askdirectory(title="Select the main folder containing EXR sequences")
        if folder:
            self.folder_path.set(folder)

    def start_processing(self):
        main_folder = self.folder_path.get()
        compression = self.compression.get()
        rgb_mode = self.rgb_mode.get()

        if not main_folder:
            messagebox.showerror("Error", "Please select a folder.")
            return

        self.process_button.config(state=tk.DISABLED)
        self.stop_event.clear()
        
        processing_args = {
            'folder': main_folder,
            'compression': compression,
            'rgb_mode': rgb_mode,
            'matte_channel_name': self.matte_channel_name.get(),
            'num_processes': self.num_processes.get(),
            'progress_queue': self.progress_queue,
            'result_queue': self.result_queue,
            'stop_event': self.stop_event
        }
        
        self.processing_thread = threading.Thread(
            target=self.processor.process_sequences,
            kwargs=processing_args
        )
        self.processing_thread.start()
        threading.Thread(target=self.update_progress, daemon=True).start()

    def update_progress(self):
        start_time = time.time()
        processed_files = 0
        
        while not self.stop_event.is_set():
            try:
                progress = self.progress_queue.get(timeout=0.1)
                if 'timing' in progress:
                    self.timing_text.set(progress['timing'])
                else:
                    self.progress_var.set(progress['progress'])
                    self.progress_text1.set(progress['status1'])
                    self.progress_text2.set(progress['status2'])
                    if 'processed' in progress:
                        processed_files = progress['processed']
                self.master.update_idletasks()
            except queue.Empty:
                continue

        # Process finished - show completion summary
        end_time = time.time()
        total_time = end_time - start_time

        try:
            result = self.result_queue.get_nowait()
            if result.get('error_files'):
                error_count = len(result['error_files'])
                success_count = processed_files - error_count
                
                self.progress_text1.set(f"Processing completed with {error_count} errors")
                self.progress_text2.set(f"Successfully processed: {success_count} files, Failed: {error_count} files")
                self.timing_text.set(f"Total processing time: {total_time:.1f} seconds")
                
                messagebox.showwarning("Processing Errors", result['error_message'])
            else:
                self.progress_text1.set("Processing completed successfully")
                self.progress_text2.set(f"Total files processed: {processed_files}")
                self.timing_text.set(f"Total processing time: {total_time:.1f} seconds")
                
                messagebox.showinfo("Success", "All files processed successfully.")
        except queue.Empty:
            self.progress_text1.set("Processing completed with unknown status")
            self.progress_text2.set("")
            self.timing_text.set("")

        self.process_button.config(state=tk.NORMAL)