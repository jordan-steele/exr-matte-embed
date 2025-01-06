import sys
from PySide6.QtWidgets import QApplication
from src.gui.main_window import EXRProcessorGUI
from src.processing.exr_processor import EXRProcessor
from version import get_version

def main():
    app = QApplication(sys.argv)
    processor = EXRProcessor()
    window = EXRProcessorGUI(processor)
    window.setWindowTitle(f"EXR Matte Embed v{get_version()}")
    window.show()
    sys.exit(app.exec())

if __name__ == "__main__":
    main()