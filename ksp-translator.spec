# -*- mode: python ; coding: utf-8 -*-

a = Analysis(
    ['src/ksp_translator/__main__.py'],
    pathex=[],
    binaries=[],
    datas=[],
    hiddenimports=[
        'ksp_translator',
        'ksp_translator.cli',
        'ksp_translator.config',
        'ksp_translator.parser',
        'ksp_translator.scanner',
        'ksp_translator.classifier',
        'ksp_translator.translator',
        'ksp_translator.generator',
        'ksp_translator.state',
        'ksp_translator.cost',
        'click',
        'rich',
        'dotenv',
        'google.genai',
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='ksp-translator',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    icon=None,
)
