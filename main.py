import tkinter as tk
from src.gui.main_window import EXRProcessorGUI
from src.processing.exr_processor import EXRProcessor
from version import get_version

def main():
    root = tk.Tk()
    root.title(f"EXR Matte Embed v{get_version()}")
    processor = EXRProcessor()
    app = EXRProcessorGUI(root, processor)
    root.mainloop()

if __name__ == "__main__":
    main()