import sys, os
from PySide6.QtWidgets import QApplication
from PySide6.QtGui import QIcon
from src.gui.main_window import EXRProcessorGUI
from src.processing.exr_processor import EXRProcessor
from version import get_version

import qdarktheme

def resource_path(relative_path):
    """ Get absolute path to resource, works for dev and for PyInstaller """
    try:
        # PyInstaller creates a temp folder and stores path in _MEIPASS
        base_path = sys._MEIPASS
    except Exception:
        base_path = os.path.abspath(".")

    return os.path.join(base_path, relative_path)

def main():
    app = QApplication(sys.argv)
    qdarktheme.setup_theme("auto")
    processor = EXRProcessor()
    window = EXRProcessorGUI(processor)
    window.setWindowTitle(f"EXR Matte Embed v{get_version()}")
    window.setWindowIcon(QIcon(resource_path("images/icon.ico")))
    window.show()
    sys.exit(app.exec())

if __name__ == "__main__":
    main()