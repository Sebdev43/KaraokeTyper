"""Setup script for KaraokeTyper."""

from setuptools import setup, find_packages
from pathlib import Path

# Read version from __version__.py
version_file = Path(__file__).parent / 'karaoketyper' / '__version__.py'
version = {}
with open(version_file) as f:
    exec(f.read(), version)

# Read README for long description
readme_file = Path(__file__).parent / 'README.md'
with open(readme_file, encoding='utf-8') as f:
    long_description = f.read()

setup(
    name='karaoketyper',
    version=version['__version__'],
    author=version['__author__'],
    description=version['__description__'],
    long_description=long_description,
    long_description_content_type='text/markdown',
    url='https://github.com/yourusername/karaoketyper',
    packages=find_packages(),
    classifiers=[
        'Development Status :: 3 - Alpha',
        'Intended Audience :: End Users/Desktop',
        'Topic :: Multimedia :: Sound/Audio :: Speech',
        'License :: OSI Approved :: GNU General Public License v3 (GPLv3)',
        'Programming Language :: Python :: 3',
        'Programming Language :: Python :: 3.12',
        'Operating System :: POSIX :: Linux',
        'Environment :: X11 Applications :: GTK',
    ],
    python_requires='>=3.12',
    install_requires=[
        'vosk>=0.3.45',
        'sounddevice>=0.4.6',
        'numpy>=1.24.3',
        'PyGObject>=3.46.0',
        'pynput>=1.7.6',
    ],
    extras_require={
        'dev': [
            'pytest>=7.4.0',
            'pytest-cov>=4.1.0',
            'black>=23.7.0',
            'flake8>=6.1.0',
            'mypy>=1.5.0',
        ],
    },
    entry_points={
        'console_scripts': [
            'karaoketyper=karaoketyper.__main__:main',
        ],
    },
    include_package_data=True,
    zip_safe=False,
)
