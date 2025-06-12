from PySide6.QtWidgets import (QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
                              QLabel, QLineEdit, QPushButton, QComboBox, 
                              QSpinBox, QProgressBar, QFileDialog,
                              QMessageBox, QSplitter, QTreeWidget, QTreeWidgetItem,
                              QGroupBox, QScrollArea, QFrame)
from PySide6.QtCore import Qt, QThread, Signal, QTimer
from PySide6.QtGui import QIcon, QFont
import threading
import queue
import multiprocessing
from ..utils.config import Config
import time, sys, os

class ScanWorker(QThread):
    scanCompleted = Signal(dict)
    
    def __init__(self, processor, folder_path):
        super().__init__()
        self.processor = processor
        self.folder_path = folder_path
    
    def run(self):
        try:
            # Scan using the new flexible approach
            pairs, warnings = self.processor.find_matching_pairs(self.folder_path)
            
            # Combine results
            scan_results = {
                'pairs': pairs,
                'warnings': warnings,
                'total_sequences': len(pairs),
                'total_files': sum(len(pair['base_files']) for pair in pairs)
            }
            
            self.scanCompleted.emit(scan_results)
            
        except Exception as e:
            self.scanCompleted.emit({
                'error': True,
                'error_message': str(e),
                'pairs': [],
                'warnings': [],
                'total_sequences': 0,
                'total_files': 0
            })

class ProcessingWorker(QThread):
    progressUpdated = Signal(dict)
    finished = Signal(dict)

    def __init__(self, processor, processing_args):
        super().__init__()
        self.processor = processor
        self.processing_args = processing_args
        self.stop_event = processing_args['stop_event']

    def run(self):
        try:
            # Run the processing with pre-scanned results
            result = self.processor.process_sequences_from_cache(**self.processing_args)
            
            # Emit the finished signal with the result
            self.finished.emit(result if result else {})
            
        except Exception as e:
            # Handle any exceptions and emit finished signal with error info
            self.finished.emit({
                'error': True,
                'error_message': str(e),
                'error_files': []
            })

class EXRProcessorGUI(QMainWindow):
    def __init__(self, processor):
        super().__init__()
        self.processor = processor
        self.setMinimumSize(800, 600)
        self.resize(1100, 700)

        # Initialize variables
        self.folder_path = ""
        self.compression = "piz"
        self.matte_channel_name = "matte"
        self.num_processes = max(multiprocessing.cpu_count() // 2, 1)
        self.scan_results = None
        self.last_folder_from_config = ""  # Initialize before loading config

        # Progress tracking variables
        self.progress_queue = queue.Queue()
        self.result_queue = queue.Queue()
        self.stop_event = threading.Event()

        self.config = Config()
        self.load_config()

        # Create central widget and layout
        central_widget = QWidget()
        self.setCentralWidget(central_widget)
        
        # Create main splitter for two-panel layout
        self.main_splitter = QSplitter(Qt.Horizontal)
        main_layout = QHBoxLayout(central_widget)
        main_layout.addWidget(self.main_splitter)

        # Create GUI elements
        self.create_control_panel()
        self.create_results_panel()
        
        # Set splitter sizes (40% controls, 60% results)
        self.main_splitter.setSizes([500, 600])
        
        # Apply saved config after UI is created
        self.apply_saved_config()
        
        # Setup progress update timer
        self.progress_timer = QTimer()
        self.progress_timer.timeout.connect(self.update_progress)
        self.start_time = None

    def load_config(self):
        config_data = self.config.load()
        self.matte_channel_name = config_data['matte_channel_name']
        # Store the last folder path for later use after UI creation
        self.last_folder_from_config = config_data.get('last_folder_path', '')
        # Store the compression setting for later use after UI creation
        self.compression = config_data.get('compression', 'piz')

    def apply_saved_config(self):
        """Apply the saved configuration after UI elements are created"""
        # Apply saved folder path
        if self.last_folder_from_config and os.path.exists(self.last_folder_from_config):
            self.folder_path = self.last_folder_from_config
            self.folder_path_edit.setText(self.last_folder_from_config)
            self.scan_button.setEnabled(True)
            # Update the summary to indicate the folder was loaded from previous session
            self.summary_label.setText("Previous folder loaded. Click 'Scan Folder' to analyze sequences.")
        
        # Apply saved compression setting
        self.compression_combo.setCurrentText(self.compression)

    def on_compression_changed(self):
        """Called when compression setting changes - save to config"""
        self.save_config()

    def save_config(self):
        config_data = {
            'matte_channel_name': self.matte_channel_name_edit.text(),
            'last_folder_path': self.folder_path,
            'compression': self.compression_combo.currentText()
        }
        self.config.save(config_data)

    def create_control_panel(self):
        # Create left panel for controls
        control_widget = QWidget()
        control_layout = QVBoxLayout(control_widget)
        
        # Title for controls
        title_label = QLabel("Processing Controls")
        title_font = QFont()
        title_font.setBold(True)
        title_font.setPointSize(12)
        title_label.setFont(title_font)
        control_layout.addWidget(title_label)
        
        # Folder selection
        folder_group = QGroupBox("Source Folder")
        folder_layout = QVBoxLayout(folder_group)
        
        folder_select_layout = QHBoxLayout()
        self.folder_path_edit = QLineEdit()
        self.folder_path_edit.setReadOnly(True)
        self.folder_path_edit.setPlaceholderText("Select a folder containing EXR sequences...")
        browse_button = QPushButton("Browse")
        browse_button.clicked.connect(self.select_folder)
        folder_select_layout.addWidget(self.folder_path_edit)
        folder_select_layout.addWidget(browse_button)
        folder_layout.addLayout(folder_select_layout)
        
        # Scan button
        self.scan_button = QPushButton("Scan Folder")
        self.scan_button.clicked.connect(self.scan_folder)
        self.scan_button.setEnabled(False)
        folder_layout.addWidget(self.scan_button)
        
        control_layout.addWidget(folder_group)

        # Processing options
        options_group = QGroupBox("Processing Options")
        options_layout = QVBoxLayout(options_group)

        # Compression
        compression_layout = QHBoxLayout()
        compression_label = QLabel("Compression:")
        self.compression_combo = QComboBox()
        self.compression_combo.setMinimumWidth(150)
        self.compression_combo.addItems(self.processor.COMPRESSION_OPTIONS)
        self.compression_combo.setCurrentText(self.compression)
        self.compression_combo.currentTextChanged.connect(self.on_compression_changed)
        compression_layout.addWidget(compression_label)
        compression_layout.addWidget(self.compression_combo)
        compression_layout.addStretch()
        options_layout.addLayout(compression_layout)

        # Matte Channel Name
        matte_layout = QHBoxLayout()
        matte_label = QLabel("Matte Channel Name:")
        self.matte_channel_name_edit = QLineEdit(self.matte_channel_name)
        self.matte_channel_name_edit.editingFinished.connect(self.save_config)
        matte_layout.addWidget(matte_label)
        matte_layout.addWidget(self.matte_channel_name_edit)
        matte_layout.addStretch()
        options_layout.addLayout(matte_layout)

        # Number of Processes
        process_layout = QHBoxLayout()
        process_label = QLabel("Number of Processes:")
        self.process_spinbox = QSpinBox()
        self.process_spinbox.setMinimumWidth(70)
        self.process_spinbox.setRange(1, multiprocessing.cpu_count())
        self.process_spinbox.setValue(self.num_processes)
        process_layout.addWidget(process_label)
        process_layout.addWidget(self.process_spinbox)
        process_layout.addStretch()
        options_layout.addLayout(process_layout)

        control_layout.addWidget(options_group)

        # Process Button
        self.process_button = QPushButton("Process Sequences")
        self.process_button.clicked.connect(self.start_processing)
        self.process_button.setEnabled(False)
        control_layout.addWidget(self.process_button)

        # Progress section
        progress_group = QGroupBox("Progress")
        progress_layout = QVBoxLayout(progress_group)
        
        self.progress_bar = QProgressBar()
        self.progress_label1 = QLabel("Ready to scan...")
        self.progress_label2 = QLabel("")
        self.timing_label = QLabel("")
        
        progress_layout.addWidget(self.progress_bar)
        progress_layout.addWidget(self.progress_label1)
        progress_layout.addWidget(self.progress_label2)
        progress_layout.addWidget(self.timing_label)
        
        control_layout.addWidget(progress_group)
        control_layout.addStretch()

        self.main_splitter.addWidget(control_widget)

    def create_results_panel(self):
        # Create right panel for scan results
        results_widget = QWidget()
        results_layout = QVBoxLayout(results_widget)
        
        # Title for results
        title_label = QLabel("Scan Results")
        title_font = QFont()
        title_font.setBold(True)
        title_font.setPointSize(12)
        title_label.setFont(title_font)
        results_layout.addWidget(title_label)
        
        # Summary info
        self.summary_label = QLabel("Select and scan a folder to see EXR sequences and their matte files")
        self.summary_label.setStyleSheet("QLabel { color: #666; font-style: italic; }")
        results_layout.addWidget(self.summary_label)
        
        # Results tree
        self.results_tree = QTreeWidget()
        self.results_tree.setHeaderLabels(["Sequence", "Type", "Files"])
        self.results_tree.setAlternatingRowColors(True)
        self.results_tree.setRootIsDecorated(True)
        
        # Set column resize modes
        header = self.results_tree.header()
        header.setStretchLastSection(False)
        header.setSectionResizeMode(0, header.ResizeMode.Interactive)  # Sequence column - manually resizable
        header.setSectionResizeMode(1, header.ResizeMode.Interactive)  # Type column - manually resizable  
        header.setSectionResizeMode(2, header.ResizeMode.Interactive)  # Files column - manually resizable
        
        # Set reasonable default column widths
        header.resizeSection(0, 300)  # Sequence column default width
        header.resizeSection(1, 200)  # Type column default width
        header.resizeSection(2, 80)   # Files column default width
        
        results_layout.addWidget(self.results_tree)
        
        # Warnings section
        self.warnings_label = QLabel("")
        self.warnings_label.setStyleSheet("QLabel { color: #e67e22; }")
        self.warnings_label.setWordWrap(True)
        results_layout.addWidget(self.warnings_label)

        self.main_splitter.addWidget(results_widget)

    def select_folder(self):
        folder = QFileDialog.getExistingDirectory(self, "Select the main folder containing EXR sequences")
        if folder:
            self.folder_path = folder
            self.folder_path_edit.setText(folder)
            self.scan_button.setEnabled(True)
            self.process_button.setEnabled(False)
            self.results_tree.clear()
            self.summary_label.setText("Folder selected. Click 'Scan Folder' to analyze sequences.")
            self.warnings_label.setText("")
            self.scan_results = None
            
            # Save the selected folder to config
            self.save_config()

    def scan_folder(self):
        if not self.folder_path:
            return
            
        self.scan_button.setEnabled(False)
        self.progress_label1.setText("Scanning folder for EXR sequences...")
        self.progress_label2.setText("")
        self.progress_bar.setValue(0)
        
        # Start scan worker
        self.scan_worker = ScanWorker(self.processor, self.folder_path)
        self.scan_worker.scanCompleted.connect(self.scan_completed)
        self.scan_worker.start()

    def scan_completed(self, results):
        self.scan_button.setEnabled(True)
        self.scan_results = results
        
        if results.get('error'):
            QMessageBox.critical(
                self,
                "Scan Error",
                f"An error occurred during scanning:\n{results.get('error_message', 'Unknown error')}"
            )
            return

        # Update summary
        total_sequences = results['total_sequences']
        total_files = results['total_files']
        
        if total_sequences == 0:
            self.summary_label.setText("No matching EXR sequences found in the selected folder.")
            self.process_button.setEnabled(False)
            return
        
        summary_text = f"Found {total_sequences} sequence(s) with {total_files} total files to process"
        self.summary_label.setText(summary_text)
        
        # Populate results tree
        self.populate_results_tree(results)
        
        # Show warnings if any
        if results['warnings']:
            warning_text = "Warnings:\n" + "\n".join(results['warnings'])
            self.warnings_label.setText(warning_text)
        else:
            self.warnings_label.setText("")
        
        # Enable process button
        self.process_button.setEnabled(True)
        self.progress_label1.setText("Scan completed. Ready to process.")

    def populate_results_tree(self, results):
        self.results_tree.clear()
        
        # Group sequences by type
        single_channel_sequences = []
        multi_channel_sequences = []
        
        for pair in results['pairs']:
            if len(pair['channels']) == 1 and 'base' in pair['channels']:
                single_channel_sequences.append(pair)
            else:
                multi_channel_sequences.append(pair)
        
        # Add single channel sequences
        if single_channel_sequences:
            single_root = QTreeWidgetItem(self.results_tree, ["Single Channel Matte Sequences", "", ""])
            single_root.setExpanded(True)
            
            for pair in single_channel_sequences:
                base_name = os.path.basename(pair['base_folder'])
                file_count = len(pair['base_files'])
                
                sequence_item = QTreeWidgetItem(single_root, [
                    base_name, 
                    "Single Channel Matte", 
                    str(file_count)
                ])
                
                # Add details as children
                QTreeWidgetItem(sequence_item, [f"  → Source: {base_name}", "", ""])
                matte_folder = os.path.basename(pair['matte_folders']['base'])
                QTreeWidgetItem(sequence_item, [f"  → Matte: {matte_folder}", "", ""])
        
        # Add multi-channel sequences
        if multi_channel_sequences:
            multi_root = QTreeWidgetItem(self.results_tree, ["Multi-Channel Matte Sequences", "", ""])
            multi_root.setExpanded(True)
            
            for pair in multi_channel_sequences:
                base_name = os.path.basename(pair['base_folder'])
                file_count = len(pair['base_files'])
                
                sequence_item = QTreeWidgetItem(multi_root, [
                    base_name, 
                    pair['sequence_type'], 
                    str(file_count)
                ])
                
                # Add details as children
                QTreeWidgetItem(sequence_item, [f"  → Source: {base_name}", "", ""])
                for channel in sorted(pair['channels']):
                    matte_folder = os.path.basename(pair['matte_folders'][channel])
                    if channel == 'base':
                        display_name = 'matte'
                    elif channel.lower() in ['r', 'g', 'b', 'a']:
                        # Show the conflict-resolved name
                        display_name = f'matte.matte_{channel.lower()}'
                    else:
                        display_name = f'matte.{channel}'
                    QTreeWidgetItem(sequence_item, [f"  → {display_name}: {matte_folder}", "", ""])

        # Column resizing is handled by the header resize modes set in create_results_panel()

    def start_processing(self):
        if not self.scan_results or self.scan_results['total_sequences'] == 0:
            QMessageBox.critical(self, "Error", "No sequences found to process.")
            return

        self.process_button.setEnabled(False)
        self.stop_event.clear()
        self.start_time = time.time()
        
        processing_args = {
            'scan_results': self.scan_results,
            'compression': self.compression_combo.currentText(),
            'matte_channel_name': self.matte_channel_name_edit.text(),
            'num_processes': self.process_spinbox.value(),
            'progress_queue': self.progress_queue,
            'result_queue': self.result_queue,
            'stop_event': self.stop_event
        }
        
        self.worker = ProcessingWorker(self.processor, processing_args)
        self.worker.finished.connect(self.processing_finished)
        self.worker.start()
        
        self.progress_timer.start(100)  # Update progress every 100ms

    def update_progress(self):
        try:
            while True:
                progress = self.progress_queue.get_nowait()
                if 'timing' in progress:
                    self.timing_label.setText(progress['timing'])
                else:
                    self.progress_bar.setValue(int(progress['progress']))
                    self.progress_label1.setText(progress['status1'])
                    self.progress_label2.setText(progress['status2'])
        except queue.Empty:
            pass

    def processing_finished(self):
        self.progress_timer.stop()
        self.process_button.setEnabled(True)
        
        try:
            result = self.result_queue.get_nowait()
            print(result)
            
            if result.get('error'):
                QMessageBox.critical(
                    self,
                    "Processing Error",
                    f"An error occurred during processing:\n{result.get('error_message', 'Unknown error')}"
                )
            elif result.get('error_files') or result.get('warnings'):
                status_message = ""
                if result.get('warnings'):
                    status_message = "Processing completed with warnings"
                    if result.get('error_files'):
                        status_message += " and errors"
                else:
                    status_message = "Processing completed with errors"
                    
                QMessageBox.warning(
                    self,
                    status_message,
                    result['error_message']
                )
            else:
                QMessageBox.information(
                    self,
                    "Success",
                    "All files processed successfully."
                )
        except queue.Empty:
            QMessageBox.warning(
                self,
                "Nothing Found to Process",
                "Nothing Found to Process"
            )
        finally:
            total_time = time.time() - self.start_time
            self.timing_label.setText(f"Total processing time: {total_time:.1f} seconds")
