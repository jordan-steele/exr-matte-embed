<img src="images/icon.png" alt="EXR Matte Embed Logo" width="128" height="128">

# EXR Matte Embed

A Python-based tool for batch embedding matte/mask sequences into EXR files. This tool automatically combines your base EXR sequences with corresponding matte sequences, embedding the mattes as additional channels within the original EXRs using flexible, semantic channel naming.

![EXR Matte Embed Interface](images/screenshot.png)

## Features

- **Flexible Matte Detection**: Automatically detects unlimited matte channels using semantic naming
- **Semantic Channel Names**: Creates meaningful channel names like `matte.screen`, `matte.fg`, `matte.hero`
- **Batch Processing**: Process entire sequences of EXR files efficiently
- **Smart Conflict Resolution**: Automatically handles naming conflicts with standard EXR channels
- **Configurable Settings**: Customizable matte channel naming and compression options
- **Multi-threaded Processing**: Optimal performance with configurable process count
- **Real-time Scanning**: Preview sequences and channels before processing
- **Progress Tracking**: Detailed progress with time estimates and file counts
- **Settings Persistence**: Remembers your folder paths and preferences between sessions
- **Cross-platform Support**: Works on Windows, macOS, and Linux

## Installation

### Download Release
The easiest way to get started is to download the pre-built application for your platform from the [Releases](https://github.com/yourusername/exr-matte-embed/releases) page:
- Windows: Download and run the `.exe` installer
- macOS: Download and mount the `.dmg` file, then drag the application to your Applications folder

<details>
<summary><b>Alternatively: Build from Source</b></summary>
If you prefer to run from source:

1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/exr-matte-embed.git
   cd exr-matte-embed
   ```

2. Create and activate a virtual environment (recommended):
   ```bash
   # Windows
   python -m venv venv
   venv\Scripts\activate

   # macOS/Linux
   python3 -m venv venv
   source venv/bin/activate
   ```

3. Install dependencies:
   ```bash
   pip install -r requirements.txt
   ```

4. Run the application:
   ```bash
   python main.py
   ```
</details>

## macOS Security Notes

When running the application on macOS, you may encounter additional security measures since the application is not signed with an Apple Developer Certificate. To run the application:

Method 1:
1. Right-click (or Control-click) the application
2. Select "Open" from the context menu
3. Click "Open" in the security dialog that appears

Alternative method:
1. Navigate to System Settings > Privacy & Security
2. Locate the security message about the blocked application
3. Click "Open Anyway" to grant permission

Note: These security prompts are part of macOS's Gatekeeper protection system. As this is an open-source application, you can review all source code in this repository to verify its safety and functionality.

## Usage

### Basic Workflow

1. **Launch the Application**:
   ```bash
   python main.py
   ```

2. **Select Source Folder**: Choose the main folder containing your EXR sequences and matte folders

3. **Scan Sequences**: Click "Scan Folder" to analyze your sequences and preview the matte channels that will be embedded

4. **Configure Settings**: Adjust compression and matte channel naming options as needed

5. **Process**: Click "Process Sequences" to begin embedding mattes into your EXR files

### Folder Structure & Naming Conventions

The tool uses flexible wildcard detection based on the `_matte` suffix pattern. Any folder ending with `_matte` followed by an optional identifier will be detected as a matte source.

#### Single Matte Channel
```
main_folder/
├── SHOT_001_v001/
│   ├── SHOT_001_v001.0001.exr
│   ├── SHOT_001_v001.0002.exr
│   └── ...
└── SHOT_001_v001_matte/
    ├── SHOT_001_v001_matte.0001.exr
    ├── SHOT_001_v001_matte.0002.exr
    └── ...
```
**Result**: Creates `matte` channel

#### Multi-Channel Mattes with Semantic Names
```
main_folder/
├── SHOT_002_v001/
│   ├── SHOT_002_v001.0001.exr
│   └── SHOT_002_v001.0002.exr
├── SHOT_002_v001_matteScreen/
├── SHOT_002_v001_matteFG/
├── SHOT_002_v001_matteBG/
└── SHOT_002_v001_matteHero/
```
**Result**: Creates channels `matte.screen`, `matte.fg`, `matte.bg`, `matte.hero`

#### Mixed Configurations
```
main_folder/
├── SHOT_003_v001/
└── SHOT_003_v001_matte/          # Base matte channel
└── SHOT_003_v001_matteHero/      # Additional hero matte
```
**Result**: Creates channels `matte`, `matte.hero`

### Automatic Conflict Resolution

The tool automatically handles channel names that would conflict with standard EXR channels:

| Folder Name | Standard Result | Conflict-Resolved Result |
|-------------|----------------|-------------------------|
| `_matteR` | `matte.r` ❌ | `matte.matte_r` ✅ |
| `_matteG` | `matte.g` ❌ | `matte.matte_g` ✅ |
| `_matteB` | `matte.b` ❌ | `matte.matte_b` ✅ |
| `_matteA` | `matte.a` ❌ | `matte.matte_a` ✅ |

This prevents interference with the main R, G, B, A color channels in your EXR files.

### Output Structure

Processed files are saved in a new folder with the suffix `_embedded`:
```
main_folder/
├── SHOT_001_v001/                 # Original sequence
├── SHOT_001_v001_matte/           # Original matte
└── SHOT_001_v001_embedded/        # ← New embedded sequence
    ├── SHOT_001_v001.0001.exr     # Contains original channels + matte channels
    └── SHOT_001_v001.0002.exr
```

### Compression Options

Available compression methods:
- **none**: No compression
- **rle**: Run-length encoding
- **zip**: ZIP compression
- **zips**: ZIP compression with scanline prediction
- **piz**: Wavelet compression (default, recommended)
- **pxr24**: Lossy 24-bit float compression
- **b44**: Lossy 4x4 block compression
- **b44a**: B44 with alpha channel compression
- **dwaa**: DWAA compression

### Advanced Features

#### Settings Persistence
The application automatically saves and restores:
- Last used folder path
- Compression preference
- Matte channel naming settings

#### Flexible Channel Limits
- **No artificial limits**: Embed as many matte channels as needed
- **Semantic naming**: Channel names reflect their actual purpose
- **Automatic detection**: No manual configuration required

#### Preview and Validation
- **Real-time scanning**: See exactly what will be processed before starting
- **File count validation**: Ensures matte sequences match base sequences
- **Conflict detection**: Preview resolved channel names in the interface

## Technical Details

### Channel Naming Logic
1. `_matte` → `{matte_channel_name}` (default: "matte")
2. `_matte{suffix}` → `{matte_channel_name}.{suffix.lowercase}`
3. Special conflict cases (R, G, B, A) → `{matte_channel_name}.matte_{suffix.lowercase}`

### Supported File Types
- **Input**: EXR sequences (any standard EXR format)
- **Output**: EXR with embedded matte channels (preserves all original channels)
- **Matte Sources**: Single-channel EXR files (uses R channel)

## License

Distributed under the MIT License. See `LICENSE` for more information.

## Acknowledgments

- [OpenEXR](https://github.com/AcademySoftwareFoundation/openexr) for the underlying EXR file handling
- [PySide6](https://doc.qt.io/qtforpython/) for the modern GUI framework
