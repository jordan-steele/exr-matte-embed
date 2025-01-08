from PySide6.QtWidgets import (QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
                              QLabel, QLineEdit, QPushButton, QComboBox, 
                              QCheckBox, QSpinBox, QProgressBar, QFileDialog,
                              QMessageBox, QToolTip)
from PySide6.QtCore import Qt, QThread, Signal, QTimer
from PySide6.QtGui import QIcon
import threading
import queue
import multiprocessing
from ..utils.config import Config
import time, sys, os

def resource_path(relative_path):
    """ Get absolute path to resource, works for dev and for PyInstaller """
    try:
        # PyInstaller creates a temp folder and stores path in _MEIPASS
        base_path = sys._MEIPASS
    except Exception:
        base_path = os.path.abspath(".")

    return os.path.join(base_path, relative_path)


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
            # Run the processing
            result = self.processor.process_sequences(**self.processing_args)
            
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
        self.setWindowIcon(QIcon(resource_path("images/icon.ico")))
        self.processor = processor
        self.setFixedSize(726, 400)

        # Initialize variables
        self.folder_path = ""
        self.compression = "piz"
        self.rgb_mode = False
        self.matte_channel_name = "matte"
        self.num_processes = max(multiprocessing.cpu_count() // 2, 1)

        # Progress tracking variables
        self.progress_queue = queue.Queue()
        self.result_queue = queue.Queue()
        self.stop_event = threading.Event()

        self.config = Config()
        self.load_config()

        # Create central widget and layout
        central_widget = QWidget()
        self.setCentralWidget(central_widget)
        self.layout = QVBoxLayout(central_widget)

        # Create GUI elements
        self.create_widgets()
        
        # Setup progress update timer
        self.progress_timer = QTimer()
        self.progress_timer.timeout.connect(self.update_progress)
        self.start_time = None

    def load_config(self):
        config_data = self.config.load()
        self.matte_channel_name = config_data['matte_channel_name']

    def save_config(self):
        config_data = {
            'matte_channel_name': self.matte_channel_name_edit.text()
        }
        self.config.save(config_data)

    def create_widgets(self):
        # Folder selection
        folder_layout = QHBoxLayout()
        folder_label = QLabel("Select Folder:")
        self.folder_path_edit = QLineEdit()
        self.folder_path_edit.setReadOnly(True)
        browse_button = QPushButton("Browse")
        browse_button.clicked.connect(self.select_folder)
        folder_layout.addWidget(folder_label)
        folder_layout.addWidget(self.folder_path_edit)
        folder_layout.addWidget(browse_button)
        self.layout.addLayout(folder_layout)

        # Compression
        compression_layout = QHBoxLayout()
        compression_label = QLabel("Compression:")
        self.compression_combo = QComboBox()
        self.compression_combo.setMinimumWidth(150)
        self.compression_combo.addItems(self.processor.COMPRESSION_OPTIONS)
        self.compression_combo.setCurrentText(self.compression)
        compression_layout.addWidget(compression_label)
        compression_layout.addWidget(self.compression_combo)
        compression_layout.addStretch()
        self.layout.addLayout(compression_layout)

        # RGB Matte Mode
        rgb_layout = QHBoxLayout()
        self.rgb_checkbox = QCheckBox("RGB Matte Mode")
        help_text = ("RGB Matte Mode expects source files to be named with _matteR, _matteG, _matteB,\n"
                    "and _matteA suffixes. It will embed four separate matte channels from these files.\n\n"
                    "For example: SHOW_100_010_020_v001_matteR/frame.001.exr, SHOW_100_010_020_v001_matteG/frame.001.exr, etc.")
        self.rgb_checkbox.setToolTip(help_text)

        # Create a help label with custom styling
        help_label = QLabel("(?)")
        help_label.setStyleSheet("""
            QLabel {
                color: #666;
                font-size: 12px;
                padding: 0 5px;
            }
        """)
        help_label.setToolTip(help_text)

        rgb_layout.addWidget(self.rgb_checkbox)
        rgb_layout.addWidget(help_label)
        rgb_layout.addStretch()
        self.layout.addLayout(rgb_layout)

        # Matte Channel Name
        matte_layout = QHBoxLayout()
        matte_label = QLabel("Matte Channel Name:")
        self.matte_channel_name_edit = QLineEdit(self.matte_channel_name)
        self.matte_channel_name_edit.editingFinished.connect(self.save_config)
        matte_layout.addWidget(matte_label)
        matte_layout.addWidget(self.matte_channel_name_edit)
        matte_layout.addStretch()
        self.layout.addLayout(matte_layout)

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
        self.layout.addLayout(process_layout)

        # Process Button
        self.process_button = QPushButton("Process")
        self.process_button.clicked.connect(self.start_processing)
        self.layout.addWidget(self.process_button, alignment=Qt.AlignCenter)

        # Progress Bar and Labels
        self.progress_bar = QProgressBar()
        self.progress_label1 = QLabel()
        self.progress_label2 = QLabel()
        self.timing_label = QLabel()
        
        self.layout.addWidget(self.progress_bar)
        self.layout.addWidget(self.progress_label1)
        self.layout.addWidget(self.progress_label2)
        self.layout.addWidget(self.timing_label)
        
        self.layout.addStretch()

    def show_rgb_help(self):
        help_text = ("RGB Matte Mode expects source files to be named with _matteR, _matteG, _matteB,\n"
                    "and _matteA suffixes. It will embed four separate matte channels from these files.\n\n"
                    "For example: SHOW_100_010_020_v001_matteR/frame.001.exr, SHOW_100_010_020_v001_matteG/frame.001.exr, etc.")
        QMessageBox.information(self, "RGB Matte Mode Help", help_text)

    def select_folder(self):
        folder = QFileDialog.getExistingDirectory(self, "Select the main folder containing EXR sequences")
        if folder:
            self.folder_path = folder
            self.folder_path_edit.setText(folder)

    def start_processing(self):
        if not self.folder_path:
            QMessageBox.critical(self, "Error", "Please select a folder.")
            return

        self.process_button.setEnabled(False)
        self.stop_event.clear()
        self.start_time = time.time()
        
        processing_args = {
            'folder': self.folder_path,
            'compression': self.compression_combo.currentText(),
            'rgb_mode': self.rgb_checkbox.isChecked(),
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