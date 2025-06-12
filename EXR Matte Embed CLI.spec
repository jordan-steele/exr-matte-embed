# -*- mode: python ; coding: utf-8 -*-
# PyInstaller spec for CLI version - minimal dependencies

a = Analysis(
    ['cli_main.py'],
    pathex=[],
    binaries=[],
    datas=[],
    hiddenimports=['numpy', 'multiprocessing'],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='exr-matte-embed-cli',
    debug=False,
    bootloader_ignore_signals=False,
    strip=True,  # Strip debug symbols
    upx=True,    # Use UPX compression
    upx_exclude=[],
    console=True,  # CLI app
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
